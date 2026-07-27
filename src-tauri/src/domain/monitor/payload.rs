// serde：结构体的序列化与反序列化派生宏。
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::MAX_NATIVE_HOOK_INPUT_BYTES;

/// 本机 Hook 接口的唯一正文契约。设备展示文案、图片、用户名等数据不属于
/// AI Hook 传输层，由常驻 `AIMonitor` 的状态机和 Profile 在后续设备转发阶段补齐。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MinimalHookPayload {
    pub hook_event_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// 从各命令型 AI 客户端写入 stdin 的原生 JSON 中只提取状态机必需字段。
/// 未知字段（`prompt`、`transcript`、`tool_input/output` 等）在进入 HTTP 边界前丢弃。
pub fn minimize_native_hook_payload(
    native_json: &[u8],
    configured_event: &str,
) -> Result<MinimalHookPayload, String> {
    if native_json.is_empty() || native_json.len() > MAX_NATIVE_HOOK_INPUT_BYTES {
        return Err("AI Hook 原始输入为空或过大".to_owned());
    }
    let source = serde_json::from_slice::<Value>(native_json)
        .map_err(|error| format!("AI Hook 原始 JSON 无效：{error}"))?;
    let source = source
        .as_object()
        .ok_or_else(|| "AI Hook 原始 JSON 的根节点必须是对象".to_owned())?;
    let body_event = string_field(source, &["hook_event_name", "type"]);
    if body_event
        .as_deref()
        .is_some_and(|event| event.trim() != configured_event)
    {
        return Err("AI Hook 原始事件与配置事件不一致".to_owned());
    }
    Ok(MinimalHookPayload {
        hook_event_name: configured_event.to_owned(),
        // WorkBuddy Desktop 的原生 Hook 上下文使用 camelCase；CodeBuddy CLI
        // 和其他兼容客户端仍使用 snake_case。两种形式必须在 relay 边界统一，
        // 否则 Stop 会落入默认会话，留下永远处于运行中的真实会话。
        session_id: string_field(
            source,
            &[
                "session_id",
                "sessionId",
                "conversation_id",
                "conversationId",
            ],
        ),
        turn_id: string_field(
            source,
            &["turn_id", "turnId", "generation_id", "generationId"],
        ),
        status: scalar_field(source, "status"),
    })
}

fn string_field(source: &Map<String, Value>, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| source.get(*name).and_then(Value::as_str).map(str::to_owned))
}

fn scalar_field(source: &Map<String, Value>, name: &str) -> Option<String> {
    match source.get(name) {
        Some(Value::String(value)) => Some(value.clone()),
        Some(Value::Bool(value)) => Some(value.to_string()),
        Some(Value::Number(value)) => Some(value.to_string()),
        _ => None,
    }
}

// 仅在测试构建中编译的单元测试模块，覆盖本文件内的纯业务逻辑。
#[cfg(test)]
mod tests {
    use super::minimize_native_hook_payload;

    #[test]
    fn native_hook_payload_is_reduced_to_the_state_machine_envelope() {
        let prompt = "包含中文、\\Windows\\路径和 \"引号\" 的长提示".repeat(100);
        let native = serde_json::json!({
            "hook_event_name": "PostToolUse",
            "conversation_id": "session-1",
            "generation_id": "turn-9",
            "status": 500,
            "prompt": prompt,
            "tool_input": { "command": "echo large" },
            "tool_output": "x".repeat(8_000),
            "transcript_path": "C:\\Users\\tester\\session.jsonl"
        });
        let native_bytes = serde_json::to_vec(&native).unwrap();

        let payload = minimize_native_hook_payload(&native_bytes, "PostToolUse").unwrap();
        let minimized = serde_json::to_vec(&payload).unwrap();

        assert_eq!(payload.hook_event_name, "PostToolUse");
        assert_eq!(payload.session_id.as_deref(), Some("session-1"));
        assert_eq!(payload.turn_id.as_deref(), Some("turn-9"));
        assert_eq!(payload.status.as_deref(), Some("500"));
        assert!(native_bytes.len() > 10_000);
        assert!(minimized.len() < 150);
        assert!(!String::from_utf8(minimized).unwrap().contains("prompt"));
    }

    #[test]
    fn workbuddy_camel_case_context_is_normalized() {
        let native = serde_json::json!({
            "hook_event_name": "Stop",
            "sessionId": "workbuddy-session-1",
            "turnId": "workbuddy-turn-2",
            "prompt": "must not leave the relay boundary"
        });

        let payload =
            minimize_native_hook_payload(&serde_json::to_vec(&native).unwrap(), "Stop").unwrap();

        assert_eq!(payload.session_id.as_deref(), Some("workbuddy-session-1"));
        assert_eq!(payload.turn_id.as_deref(), Some("workbuddy-turn-2"));
    }
}
