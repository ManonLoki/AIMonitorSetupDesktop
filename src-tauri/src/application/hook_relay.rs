//! 命令型 AI Hook 的轻量入口。
//!
//! `AIMonitor` 可执行文件带 `--aimonitor-hook-relay <tool> <event>` 启动时只运行
//! 本模块，不初始化 Tauri。它从 stdin 的原生 JSON 中提取最小状态上下文，再把
//! 小信封提交给已运行桌面实例的环回 listener。

use std::{
    ffi::{OsStr, OsString},
    io::{self, Read, Write},
    iter, thread,
    time::Duration,
};

use anyhow::Context;
use clap::Parser;
use reqwest::blocking::Client;

use crate::domain::monitor::{
    AiTool, DEFAULT_HOOK_RELAY_PORT, MAX_NATIVE_HOOK_INPUT_BYTES, managed_hook_marker,
    minimize_native_hook_payload, tool_from_slug,
};

pub const HOOK_RELAY_ARGUMENT: &str = "--aimonitor-hook-relay";
const LOCAL_RELAY_RETRY_COUNT: u8 = 5;
const LOCAL_RELAY_RETRY_DELAY: Duration = Duration::from_secs(1);
// 连接本机 listener 应该几乎瞬时完成；超时说明监听进程还没起来或已经卡死。
const LOCAL_RELAY_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
// 整个请求（含 listener 入队处理）的超时，略宽松于连接超时。
const LOCAL_RELAY_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

// Hook relay 子进程模式的命令行参数：`--aimonitor-hook-relay <tool> <event>
// --managed-by <marker>`。由 clap 负责解析与校验参数形状，值本身的合法性
// （工具是否存在、marker 是否匹配该工具）在 `validate_relay_arguments` 中检查。
#[derive(Debug, Parser, PartialEq, Eq)]
#[command(name = "AIMonitor")]
struct HookRelayArguments {
    #[arg(long = "aimonitor-hook-relay", value_name = "AI_TOOL")]
    tool_slug: String,
    #[arg(value_name = "EVENT")]
    event: String,
    #[arg(long = "managed-by", value_name = "MARKER")]
    managed_by: String,
}

/// 若当前进程参数是 Hook relay 模式则执行并返回退出码；否则让调用方继续启动 GUI。
pub fn run_from_process_args() -> Option<i32> {
    let arguments = parse_relay_arguments(std::env::args_os())?;
    let arguments = match arguments {
        Ok(arguments) => arguments,
        Err(error) => {
            let exit_code = error.exit_code();
            let _ = error.print();
            return Some(exit_code);
        }
    };
    let tool = match validate_relay_arguments(&arguments) {
        Ok(tool) => tool,
        Err(error) => {
            eprintln!("{error}");
            return Some(1);
        }
    };
    Some(relay_exit_code(
        tool,
        relay_stdin(tool, &arguments.tool_slug, &arguments.event),
    ))
}

// Copilot CLI 的 preToolUse 在命令非零退出时会拒绝工具调用。AIMonitor 是旁路
// 观测器，桌面端未运行或 listener 暂时不可达时绝不能改变 AI 工具行为，因此
// Copilot 投递失败只记录 stderr 并以成功退出；其他工具保持既有错误可见语义。
fn relay_exit_code(tool: AiTool, result: anyhow::Result<()>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(error) => {
            // `{:#}` 打印 anyhow 的完整错误链（本层 context + 底层原因），
            // 便于从 AI 工具捕获的 stderr 直接定位问题，不需要额外查日志文件。
            eprintln!("{error:#}");
            i32::from(tool != AiTool::GitHubCopilot)
        }
    }
}

// 只有当第二个参数正好是 `--aimonitor-hook-relay` 时才进入 relay 模式（返回
// `Some`）；否则返回 `None`，让调用方继续走正常的 Tauri GUI 启动路径。
// 命中 relay 模式后交给 clap 解析剩余参数，形状错误由 `Some(Err(..))` 承载。
fn parse_relay_arguments<I, T>(arguments: I) -> Option<Result<HookRelayArguments, clap::Error>>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let mut arguments = arguments.into_iter();
    let executable = arguments.next()?;
    let first_argument = arguments.next()?;
    if first_argument.clone().into() != OsStr::new(HOOK_RELAY_ARGUMENT) {
        return None;
    }
    Some(HookRelayArguments::try_parse_from(
        iter::once(executable)
            .chain(iter::once(first_argument))
            .chain(arguments),
    ))
}

/// 校验失败时的两种已知原因，供调用方按 `Display` 直接输出到 stderr。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
enum RelayValidationError {
    #[error("Hook relay 不支持 AI 工具：{0}")]
    UnsupportedTool(String),
    #[error("Hook relay 的管理标识与 AI 工具不匹配")]
    MarkerMismatch,
}

// 校验 relay 参数的语义（clap 只保证了形状）：tool_slug 必须是受支持的 AI
// 工具，且 marker 必须与该工具的 `managed_hook_marker` 完全一致，防止配置
// 文件被篡改后触发跨工具的 Hook 请求。
fn validate_relay_arguments(
    arguments: &HookRelayArguments,
) -> Result<AiTool, RelayValidationError> {
    let tool = tool_from_slug(&arguments.tool_slug)
        .ok_or_else(|| RelayValidationError::UnsupportedTool(arguments.tool_slug.clone()))?;
    let expected_marker = managed_hook_marker(tool);
    if arguments.managed_by != expected_marker {
        return Err(RelayValidationError::MarkerMismatch);
    }
    Ok(tool)
}

// 读取 AI 工具通过 stdin 传来的原生 Hook JSON，压缩成最小信封后 POST 给本机
// listener；需要 stdout JSON 的协议由下方按工具显式处理。
fn relay_stdin(tool: AiTool, tool_slug: &str, event: &str) -> anyhow::Result<()> {
    let mut native_json = Vec::new();
    io::stdin()
        .take((MAX_NATIVE_HOOK_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut native_json)
        .context("无法读取 AI Hook 原始输入")?;
    let payload =
        minimize_native_hook_payload(&native_json, event).context("无法处理 AI Hook 原始载荷")?;
    let endpoint = format!("http://127.0.0.1:{DEFAULT_HOOK_RELAY_PORT}/api/hooks/{tool_slug}");
    let client = super::net::harden_blocking_client(
        Client::builder()
            .connect_timeout(LOCAL_RELAY_CONNECT_TIMEOUT)
            .timeout(LOCAL_RELAY_REQUEST_TIMEOUT),
    )
    .build()
    .context("无法创建 Hook relay 客户端")?;
    post_minimal_payload(&client, &endpoint, event, &payload)?;
    // Cursor、Qwen Code 与 Gemini CLI 都要求 command Hook 的 stdout 是合法 JSON；
    // 其他工具成功时保持空输出，避免干扰各自的默认 Hook 决策。
    if matches!(tool, AiTool::Cursor | AiTool::QwenCode | AiTool::GeminiCli) {
        io::stdout()
            .write_all(b"{}\n")
            .context("无法写入 AI Hook JSON 响应")?;
    }
    Ok(())
}

/// `post_minimal_payload` 的两种已知失败原因；其余网络/序列化问题仍通过
/// `anyhow::Error` 的来源链（`reqwest::Error` 等）向上传播，不必逐一枚举。
#[derive(Debug, thiserror::Error)]
enum RelayDeliveryError {
    #[error("AIMonitor Hook listener 拒绝了事件：HTTP {status}")]
    Rejected { status: reqwest::StatusCode },
    #[error("无法连接 AIMonitor Hook listener")]
    ConnectFailed(#[source] reqwest::Error),
}

// 把最小信封 POST 给本机 listener；仅在连接失败（listener 可能还没起来）时
// 按固定次数/间隔重试，设备侧拒绝或其他网络错误直接返回错误不重试。
fn post_minimal_payload(
    client: &Client,
    endpoint: &str,
    event: &str,
    payload: &crate::domain::monitor::MinimalHookPayload,
) -> anyhow::Result<()> {
    let mut attempt = 0_u8;
    loop {
        match client
            .post(endpoint)
            .header("X-AIMonitor-Hook-Type", event)
            .json(&payload)
            .send()
        {
            Ok(response) if response.status().is_success() => break,
            Ok(response) => {
                return Err(RelayDeliveryError::Rejected {
                    status: response.status(),
                }
                .into());
            }
            Err(error) if error.is_connect() && attempt < LOCAL_RELAY_RETRY_COUNT => {
                attempt += 1;
                thread::sleep(LOCAL_RELAY_RETRY_DELAY);
            }
            Err(error) => return Err(RelayDeliveryError::ConnectFailed(error).into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::{Ipv4Addr, TcpListener},
        sync::mpsc,
    };

    use super::*;
    use crate::domain::monitor::{MinimalHookPayload, minimize_native_hook_payload};

    #[test]
    fn clap_parses_managed_hook_relay_arguments() {
        let parsed = parse_relay_arguments([
            "AIMonitor",
            HOOK_RELAY_ARGUMENT,
            "codex",
            "PostToolUse",
            "--managed-by",
            "AIMonitor:tool=codex",
        ])
        .unwrap()
        .unwrap();

        assert_eq!(
            parsed,
            HookRelayArguments {
                tool_slug: "codex".to_owned(),
                event: "PostToolUse".to_owned(),
                managed_by: "AIMonitor:tool=codex".to_owned(),
            }
        );
        assert_eq!(validate_relay_arguments(&parsed), Ok(AiTool::Codex));
    }

    #[test]
    fn clap_relay_accepts_every_configured_tool_slug_and_canonical_marker() {
        for (slug, tool) in [
            ("codex", AiTool::Codex),
            ("claude-code", AiTool::ClaudeCode),
            ("cursor", AiTool::Cursor),
            ("opencode", AiTool::OpenCode),
            ("workbuddy", AiTool::WorkBuddy),
            ("hermes", AiTool::Hermes),
            ("openclaw", AiTool::OpenClaw),
            ("codebuddy", AiTool::CodeBuddy),
            ("qwen-code", AiTool::QwenCode),
            ("kimi-code", AiTool::KimiCode),
            ("qoder", AiTool::Qoder),
            ("gemini-cli", AiTool::GeminiCli),
            ("github-copilot", AiTool::GitHubCopilot),
        ] {
            let marker = managed_hook_marker(tool);
            let parsed = parse_relay_arguments([
                "AIMonitor",
                HOOK_RELAY_ARGUMENT,
                slug,
                "contract-probe",
                "--managed-by",
                &marker,
            ])
            .unwrap()
            .unwrap();

            assert_eq!(validate_relay_arguments(&parsed), Ok(tool));
        }
    }

    #[test]
    fn clap_rejects_incomplete_or_extra_relay_arguments() {
        let missing_marker =
            parse_relay_arguments(["AIMonitor", HOOK_RELAY_ARGUMENT, "codex", "Stop"]).unwrap();
        assert!(missing_marker.is_err());

        let extra = parse_relay_arguments([
            "AIMonitor",
            HOOK_RELAY_ARGUMENT,
            "codex",
            "Stop",
            "--managed-by",
            "AIMonitor:tool=codex",
            "unexpected",
        ])
        .unwrap();
        assert!(extra.is_err());
    }

    #[test]
    fn non_relay_process_arguments_leave_tauri_startup_untouched() {
        assert!(parse_relay_arguments(["AIMonitor", "--silent"]).is_none());
        assert!(parse_relay_arguments(["AIMonitor"]).is_none());
    }

    #[test]
    fn relay_management_marker_must_match_the_tool() {
        let parsed = parse_relay_arguments([
            "AIMonitor",
            HOOK_RELAY_ARGUMENT,
            "codex",
            "Stop",
            "--managed-by",
            "AIMonitor:tool=cursor",
        ])
        .unwrap()
        .unwrap();

        assert_eq!(
            validate_relay_arguments(&parsed),
            Err(RelayValidationError::MarkerMismatch)
        );
    }

    #[test]
    fn copilot_delivery_failure_is_fail_open() {
        assert_eq!(
            relay_exit_code(
                AiTool::GitHubCopilot,
                Err(anyhow::anyhow!("listener offline"))
            ),
            0
        );
        assert_eq!(
            relay_exit_code(AiTool::Codex, Err(anyhow::anyhow!("listener offline"))),
            1
        );
    }

    #[test]
    fn relay_http_request_contains_only_the_minimal_envelope() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let endpoint = format!("http://{}/api/hooks/codex", listener.local_addr().unwrap());
        let (body_sender, body_receiver) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            let (header_end, content_length) = loop {
                let read = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..read]);
                let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n")
                else {
                    continue;
                };
                let header_end = header_end + 4;
                let headers = std::str::from_utf8(&request[..header_end]).unwrap();
                let content_length = headers
                    .split("\r\n")
                    .find_map(|line| {
                        line.split_once(':').and_then(|(name, value)| {
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                    })
                    .unwrap();
                break (header_end, content_length);
            };
            while request.len() < header_end + content_length {
                let read = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..read]);
            }
            body_sender
                .send(request[header_end..header_end + content_length].to_vec())
                .unwrap();
            stream
                .write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });
        let native = serde_json::to_vec(&serde_json::json!({
            "hook_event_name": "PostToolUse",
            "session_id": "session-1",
            "turn_id": "turn-2",
            "prompt": "private prompt",
            "tool_output": "x".repeat(20_000)
        }))
        .unwrap();
        let payload = minimize_native_hook_payload(&native, "PostToolUse").unwrap();
        let client = Client::builder().build().unwrap();

        post_minimal_payload(&client, &endpoint, "PostToolUse", &payload).unwrap();

        server.join().unwrap();
        let body = body_receiver.recv().unwrap();
        let received = serde_json::from_slice::<MinimalHookPayload>(&body).unwrap();
        assert_eq!(received, payload);
        assert!(body.len() < 150);
        assert!(!String::from_utf8(body).unwrap().contains("private prompt"));
    }
}
