//! 命令型 AI Hook 的轻量入口。
//!
//! `AIMonitor` 可执行文件带 `--aimonitor-hook-relay <tool> <event>` 启动时只运行
//! 本模块，不初始化 Tauri。它从 stdin 的原生 JSON 中提取最小状态上下文，再把
//! 小信封提交给已运行桌面实例的环回 listener。

use std::{
    io::{self, Read, Write},
    thread,
    time::Duration,
};

use reqwest::blocking::Client;

use crate::domain::monitor::{
    AiTool, DEFAULT_HOOK_RELAY_PORT, MAX_NATIVE_HOOK_INPUT_BYTES, minimize_native_hook_payload,
    tool_from_slug,
};

pub const HOOK_RELAY_ARGUMENT: &str = "--aimonitor-hook-relay";
const LOCAL_RELAY_RETRY_COUNT: u8 = 5;
const LOCAL_RELAY_RETRY_DELAY: Duration = Duration::from_secs(1);

/// 若当前进程参数是 Hook relay 模式则执行并返回退出码；否则让调用方继续启动 GUI。
pub fn run_from_process_args() -> Option<i32> {
    let mut arguments = std::env::args();
    let _executable = arguments.next();
    if arguments.next().as_deref() != Some(HOOK_RELAY_ARGUMENT) {
        return None;
    }
    let result = arguments
        .next()
        .ok_or_else(|| "Hook relay 缺少 AI 工具参数".to_owned())
        .and_then(|tool_slug| {
            let tool = tool_from_slug(&tool_slug)
                .ok_or_else(|| format!("Hook relay 不支持 AI 工具：{tool_slug}"))?;
            let event = arguments
                .next()
                .ok_or_else(|| "Hook relay 缺少事件类型参数".to_owned())?;
            let expected_marker = format!("AIMonitor|tool={tool_slug}");
            if arguments.next().as_deref() != Some("--managed-by")
                || arguments.next().as_deref() != Some(expected_marker.as_str())
                || arguments.next().is_some()
            {
                return Err("Hook relay 收到了多余参数".to_owned());
            }
            relay_stdin(tool, &tool_slug, &event)
        });
    Some(match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    })
}

fn relay_stdin(tool: AiTool, tool_slug: &str, event: &str) -> Result<(), String> {
    let mut native_json = Vec::new();
    io::stdin()
        .take((MAX_NATIVE_HOOK_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut native_json)
        .map_err(|error| format!("无法读取 AI Hook 原始输入：{error}"))?;
    let payload = minimize_native_hook_payload(&native_json, event)?;
    let endpoint = format!("http://127.0.0.1:{DEFAULT_HOOK_RELAY_PORT}/api/hooks/{tool_slug}");
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|error| format!("无法创建 Hook relay 客户端：{error}"))?;
    post_minimal_payload(&client, &endpoint, event, &payload)?;
    // Cursor 要求 command Hook 的 stdout 是合法 JSON；其他工具成功时保持空输出。
    if tool == AiTool::Cursor {
        io::stdout()
            .write_all(b"{}\n")
            .map_err(|error| format!("无法写入 Cursor Hook 响应：{error}"))?;
    }
    Ok(())
}

fn post_minimal_payload(
    client: &Client,
    endpoint: &str,
    event: &str,
    payload: &crate::domain::monitor::MinimalHookPayload,
) -> Result<(), String> {
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
                return Err(format!(
                    "AIMonitor Hook listener 拒绝了事件：HTTP {}",
                    response.status()
                ));
            }
            Err(error) if error.is_connect() && attempt < LOCAL_RELAY_RETRY_COUNT => {
                attempt += 1;
                thread::sleep(LOCAL_RELAY_RETRY_DELAY);
            }
            Err(error) => return Err(format!("无法连接 AIMonitor Hook listener：{error}")),
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
