// 本机 Hook 中继的 axum 入口：解析路径 slug + 请求头 + 请求体为一个最小信封
// 事件（`IncomingHookEvent`），再送进状态机 worker（`relay::worker`）的队列。
use axum::{
    extract::{Path as RoutePath, State},
    http::{HeaderMap, StatusCode as AxumStatusCode},
};

use crate::{
    application::monitor::{
        HookListenerState, IncomingHookEvent, MAX_HOOK_SESSION_ID_BYTES, MAX_HOOK_STATUS_BYTES,
        MAX_HOOK_TURN_ID_BYTES,
    },
    domain::monitor::{MinimalHookPayload, tool_from_slug},
};

use super::status::record_relay_failure;

// axum 路由处理函数：路径参数携带工具 slug，正文大小已由 DefaultBodyLimit 层拦截，
// 这里只需做协议无关的信封校验，再把解析结果送进状态机 worker 的通道。
pub(crate) async fn handle_hook_request(
    RoutePath(tool_slug): RoutePath<String>,
    State(state): State<HookListenerState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> (AxumStatusCode, &'static str) {
    let header_hook_type = headers
        .get("x-aimonitor-hook-type")
        .and_then(|value| value.to_str().ok());

    let event = match parse_hook_envelope(&tool_slug, header_hook_type, &body) {
        Ok(event) => event,
        Err(error) => {
            record_relay_failure(&state.status, error);
            return (AxumStatusCode::BAD_REQUEST, "Bad Request");
        }
    };

    // 解析成功先把待处理计数加一，再尝试送入状态机 worker 的队列。
    if let Ok(mut current) = state.status.write() {
        current.pending_count += 1;
    }
    if state.sender.send(event).is_ok() {
        // 成功入队后立刻回 202，表示已接受、异步处理。
        (AxumStatusCode::ACCEPTED, "Accepted")
    } else {
        // 工作线程已经退出导致发送失败：回滚待处理计数，返回 503，并记录失败。
        if let Ok(mut current) = state.status.write() {
            current.pending_count = current.pending_count.saturating_sub(1);
        }
        record_relay_failure(&state.status, "Hook 转发工作线程已停止".to_owned());
        (AxumStatusCode::SERVICE_UNAVAILABLE, "Service Unavailable")
    }
}

// 校验并解析一个 Hook 请求的最小信封：路径 slug 决定 AI 工具，请求体必须是唯一的
// 最小信封契约，且其中的事件名必须与可信事件头一致，避免配置事件与动态正文混淆。
fn parse_hook_envelope(
    tool_slug: &str,
    header_hook_type: Option<&str>,
    body: &[u8],
) -> Result<IncomingHookEvent, String> {
    // 根据路径 slug 确定是哪个 AI 工具发来的 Hook 请求；slug 与工具的映射统一由
    // 各 HookProtocol 的 slug() 提供，此处不再重复维护一份镜像表。
    let tool = tool_from_slug(tool_slug).ok_or_else(|| "Hook 请求中的 AI 工具无效".to_owned())?;
    // 请求体长度必须大于 0（DefaultBodyLimit 层已拦截过大的请求体）。
    if body.is_empty() {
        return Err("Hook 请求体大小无效".to_owned());
    }
    let header_hook_type =
        header_hook_type.ok_or_else(|| "Hook 请求缺少 X-AIMonitor-Hook-Type".to_owned())?;
    // 命令型工具已由轻量 relay 子进程丢弃原生大字段；插件型工具也直接构造相同的
    // 四字段信封，因此正文按唯一的最小信封契约反序列化即可。
    let payload = serde_json::from_slice::<MinimalHookPayload>(body)
        .map_err(|error| format!("Hook 请求 JSON 无效：{error}"))?;
    if payload.hook_event_name.trim() != header_hook_type.trim() {
        return Err("Hook 请求体与事件头不一致".to_owned());
    }
    let hook_type = payload.hook_event_name.trim();
    // 事件类型字符串不能为空，也不能过长。
    if hook_type.is_empty() || hook_type.len() > 128 {
        return Err("Hook 类型不能为空且不能超过 128 个字符".to_owned());
    }
    let session_id =
        normalize_hook_context_field(payload.session_id, "session_id", MAX_HOOK_SESSION_ID_BYTES)?;
    let turn_id = normalize_hook_context_field(payload.turn_id, "turn_id", MAX_HOOK_TURN_ID_BYTES)?;
    let status = normalize_hook_context_field(payload.status, "status", MAX_HOOK_STATUS_BYTES)?;
    Ok(IncomingHookEvent {
        tool,
        hook_type: hook_type.to_owned(),
        session_id,
        turn_id,
        status,
    })
}

fn normalize_hook_context_field(
    value: Option<String>,
    field: &str,
    max_bytes: usize,
) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > max_bytes {
        return Err(format!("Hook {field} 不能超过 {max_bytes} 个 UTF-8 字节"));
    }
    Ok(Some(value.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::monitor::AiTool;

    // 验证 parse_hook_envelope 能正确解析一个合法的最小 Hook 请求：
    // 最小信封和可信事件头应被成功解析出 (工具, 事件类型)。
    #[test]
    fn local_hook_request_accepts_the_minimal_envelope_and_event_header() {
        let body = br#"{"hook_event_name":"SessionStart"}"#;

        let request = parse_hook_envelope("codex", Some("SessionStart"), body).unwrap();

        // 期望解析出 Codex 工具与 SessionStart 事件类型。
        assert_eq!(
            request,
            IncomingHookEvent {
                tool: AiTool::Codex,
                hook_type: "SessionStart".to_owned(),
                session_id: None,
                turn_id: None,
                status: None,
            }
        );
    }

    // 验证规范化后的最小 Hook 信封可以携带状态机所需上下文。
    #[test]
    fn local_hook_request_accepts_native_context_and_event_header() {
        let body =
            br#"{"hook_event_name":"stop","session_id":"s-1","turn_id":"t-1","status":"cancelled"}"#;

        let parsed = parse_hook_envelope("cursor", Some("stop"), body).unwrap();

        assert_eq!(
            parsed,
            IncomingHookEvent {
                tool: AiTool::Cursor,
                hook_type: "stop".to_owned(),
                session_id: Some("s-1".to_owned()),
                turn_id: Some("t-1".to_owned()),
                status: Some("cancelled".to_owned()),
            }
        );
    }

    #[test]
    fn local_hook_request_rejects_fields_outside_the_minimal_envelope() {
        let body =
            br#"{"hook_event_name":"PostToolUse","session_id":"s-1","tool_output":"large and private"}"#;

        let error = parse_hook_envelope("codex", Some("PostToolUse"), body).unwrap_err();

        assert!(error.contains("unknown field"));
        assert!(error.contains("tool_output"));
    }

    #[test]
    fn local_hook_request_rejects_context_identifiers_that_could_bloat_the_queue() {
        let oversized_session_id = "s".repeat(MAX_HOOK_SESSION_ID_BYTES + 1);
        let body = format!(
            r#"{{"hook_event_name":"UserPromptSubmit","session_id":"{oversized_session_id}"}}"#
        );

        let error =
            parse_hook_envelope("codex", Some("UserPromptSubmit"), body.as_bytes()).unwrap_err();

        assert!(error.contains("session_id"));
        assert!(error.contains(&MAX_HOOK_SESSION_ID_BYTES.to_string()));
    }

    #[test]
    fn local_hook_request_routes_new_tool_slugs() {
        for (slug, tool) in [
            ("hermes", AiTool::Hermes),
            ("openclaw", AiTool::OpenClaw),
            ("codebuddy", AiTool::CodeBuddy),
        ] {
            let body = br#"{"hook_event_name":"probe"}"#;

            let parsed = parse_hook_envelope(slug, Some("probe"), body).unwrap();

            assert_eq!(parsed.tool, tool);
            assert_eq!(parsed.hook_type, "probe");
        }
    }
}
