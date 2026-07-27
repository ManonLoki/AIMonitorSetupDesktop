use super::{HookEvent, HookEventKind, HookProtocol, managed_hook_marker};
use crate::domain::monitor::{AiTool, DEFAULT_HOOK_RELAY_PORT, HookBehavior};

pub(super) static OPEN_CODE: OpenCodeProtocol = OpenCodeProtocol;

pub(super) struct OpenCodeProtocol;

const EVENTS: &[HookEvent] = &[
    HookEvent::new("session.created", HookEventKind::SessionStart),
    HookEvent::new("session.busy", HookEventKind::WorkStart),
    HookEvent::new(
        "tool.execute.before",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "tool.execute.after",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "permission.asked",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new("question.asked", HookEventKind::State(HookBehavior::Asking)),
    HookEvent::new("session.retry", HookEventKind::State(HookBehavior::Error)),
    HookEvent::new("session.error", HookEventKind::State(HookBehavior::Error)),
    HookEvent::new("session.idle", HookEventKind::Stop),
    HookEvent::new("session.deleted", HookEventKind::SessionEnd),
];

impl HookProtocol for OpenCodeProtocol {
    fn tool(&self) -> AiTool {
        AiTool::OpenCode
    }
    fn name(&self) -> &'static str {
        "OpenCode"
    }
    fn slug(&self) -> &'static str {
        "opencode"
    }
    fn config_filename(&self) -> &'static str {
        "plugins/aimonitor.js"
    }
    fn preview_filename(&self) -> &'static str {
        ".config/opencode/plugins/aimonitor.js"
    }
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    fn standalone_config(&self) -> Option<String> {
        let marker = managed_hook_marker(AiTool::OpenCode);
        // OpenCode 的 session.idle 在失败后的收尾阶段也会触发一次，且无法从事件本身
        // 区分是否紧跟在 retry/error 之后；用 failedSessions 记录出错会话，
        // 抑制这次误报的 idle，避免把「运行出错」误显示成「已完成」。
        Some(format!(
            r#"// {marker}
const endpoint = "http://127.0.0.1:{DEFAULT_HOOK_RELAY_PORT}/api/hooks/opencode"
const failedSessions = new Set()

const normalizedEvent = (event) => {{
  const properties = event.properties ?? {{}}
  const sessionID = properties.sessionID ?? properties.info?.id ?? null
  if (event.type === "session.status") {{
    const status = properties.status?.type
    if (status === "busy") {{
      if (sessionID) failedSessions.delete(sessionID)
      return "session.busy"
    }}
    if (status === "retry") {{
      if (sessionID) failedSessions.add(sessionID)
      return "session.retry"
    }}
    if (status === "idle") {{
      return sessionID && failedSessions.has(sessionID) ? null : "session.idle"
    }}
    return null
  }}
  if (event.type === "session.error") {{
    if (sessionID) failedSessions.add(sessionID)
    return "session.error"
  }}
  if (event.type === "session.idle" && sessionID && failedSessions.has(sessionID)) {{
    return null
  }}
  if (event.type === "session.deleted" && sessionID) failedSessions.delete(sessionID)
  const supported = new Set([
    "session.created",
    "tool.execute.before",
    "tool.execute.after",
    "permission.asked",
    "question.asked",
    "session.idle",
    "session.deleted",
  ])
  return supported.has(event.type) ? event.type : null
}}

const send = async (event) => {{
  const hookEvent = normalizedEvent(event)
  if (!hookEvent) return
  const properties = event.properties ?? {{}}
  const sessionID = properties.sessionID ?? properties.info?.id ?? null
  const status = properties.status?.type ?? null
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
        status,
      }}),
      signal: AbortSignal.timeout(3000),
    }})
  }} catch {{
    // AIMonitor 未运行时不影响 OpenCode 的任务执行。
  }}
}}

export const AIMonitorPlugin = async () => ({{
  event: async ({{ event }}) => send(event),
}})
"#
        ))
    }
}
