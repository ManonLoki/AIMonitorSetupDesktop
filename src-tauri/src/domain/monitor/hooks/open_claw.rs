use super::{HookEvent, HookEventKind, HookProtocol, managed_hook_marker};
use crate::domain::monitor::{
    AiTool, DEFAULT_HOOK_RELAY_PORT, HookBehavior, HookConfigPreview, HookWriteOutcome,
};

pub(super) static OPEN_CLAW: OpenClawProtocol = OpenClawProtocol;

pub(super) struct OpenClawProtocol;

const EVENTS: &[HookEvent] = &[
    HookEvent::new("session_start", HookEventKind::SessionStart),
    HookEvent::new("before_agent_run", HookEventKind::WorkStart),
    HookEvent::new(
        "before_tool_call",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "after_tool_call",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new("agent_end", HookEventKind::Stop),
    HookEvent::new("session_end", HookEventKind::SessionEnd),
];

impl HookProtocol for OpenClawProtocol {
    fn tool(&self) -> AiTool {
        AiTool::OpenClaw
    }

    fn name(&self) -> &'static str {
        "OpenClaw"
    }

    fn slug(&self) -> &'static str {
        "openclaw"
    }

    fn config_filename(&self) -> &'static str {
        "extensions/aimonitor/index.mjs"
    }

    fn preview_filename(&self) -> &'static str {
        ".openclaw/extensions/aimonitor/index.mjs"
    }

    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    fn event_kind(&self, event: &HookEvent, status: Option<&str>) -> HookEventKind {
        // "false" 是插件把 `event.success === false` 直接转成字符串的结果；
        // "error"/"failed" 对应插件自行归纳的 outcome 值，三者都表示失败。
        if event.name == "agent_end" && matches!(status, Some("error" | "failed" | "false")) {
            HookEventKind::State(HookBehavior::Error)
        } else {
            event.kind
        }
    }

    // 插件安装后需要显式启用、授权，并重启 Gateway 才能发现和加载。
    fn changed_write_outcome(&self) -> HookWriteOutcome {
        HookWriteOutcome::OpenClawEnableRequired
    }

    fn standalone_config(&self) -> Option<String> {
        let marker = managed_hook_marker(AiTool::OpenClaw);
        Some(format!(
            r#"// {marker}
const endpoint = "http://127.0.0.1:{DEFAULT_HOOK_RELAY_PORT}/api/hooks/openclaw"

const send = async (hookEvent, event = {{}}, ctx = {{}}) => {{
  const sessionID = ctx.sessionId ?? ctx.sessionKey ?? event.sessionId ?? event.sessionKey ?? null
  const turnID = event.runId ?? ctx.runId ?? null
  const status = hookEvent === "agent_end"
    ? (event.success === false ? "failed" : (event.outcome ?? "success"))
    : null
  try {{
    await fetch(endpoint, {{
      method: "POST",
      headers: {{
        "Content-Type": "application/json",
        "X-AIMonitor-Hook-Type": hookEvent,
      }},
      body: JSON.stringify({{
        hook_event_name: hookEvent,
        session_id: sessionID,
        turn_id: turnID,
        status,
      }}),
      signal: AbortSignal.timeout(3000),
    }})
  }} catch {{
    // AIMonitor 未运行时不影响 OpenClaw Gateway。
  }}
}}

export default {{
  id: "aimonitor",
  name: "AIMonitor",
  description: "Relay OpenClaw lifecycle state to the local AIMonitor desktop app",
  register(api) {{
    for (const hook of [
      "session_start",
      "before_agent_run",
      "before_tool_call",
      "after_tool_call",
      "agent_end",
      "session_end",
    ]) {{
      api.on(hook, (event, ctx) => send(hook, event, ctx))
    }}
  }},
}}
"#
        ))
    }

    fn auxiliary_configs(&self) -> Vec<HookConfigPreview> {
        let marker = managed_hook_marker(AiTool::OpenClaw);
        vec![
            HookConfigPreview {
                filename: "extensions/aimonitor/openclaw.plugin.json".to_owned(),
                content: format!(
                    r#"{{
  "id": "aimonitor",
  "name": "AIMonitor",
  "description": "{marker} - relay OpenClaw lifecycle state to AIMonitor",
  "version": "1.0.0",
  "activation": {{ "onStartup": true }},
  "configSchema": {{ "type": "object", "additionalProperties": false }}
}}"#
                ),
            },
            HookConfigPreview {
                filename: "extensions/aimonitor/package.json".to_owned(),
                content: format!(
                    r#"{{
  "name": "aimonitor-openclaw-plugin",
  "version": "1.0.0",
  "private": true,
  "type": "module",
  "description": "{marker}",
  "openclaw": {{ "extensions": ["./index.mjs"] }}
}}"#
                ),
            },
        ]
    }
}
