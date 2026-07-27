use serde_json::{Map, Value, json};

#[cfg(any(target_os = "macos", target_os = "linux"))]
use super::platform_command;
use super::{HookEvent, HookEventKind, HookProtocol, ManagedCommands, entry_is_managed};
use crate::domain::monitor::{AiTool, HookBehavior};

pub(super) static CURSOR: CursorProtocol = CursorProtocol;

pub(super) struct CursorProtocol;

const EVENTS: &[HookEvent] = &[
    HookEvent::new("workspaceOpen", HookEventKind::SessionStart),
    HookEvent::new("sessionStart", HookEventKind::SessionStart),
    HookEvent::new("beforeSubmitPrompt", HookEventKind::WorkStart),
    HookEvent::new(
        "afterFileEdit",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "afterShellExecution",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "afterMCPExecution",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "beforeShellExecution",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new(
        "beforeMCPExecution",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new(
        "preToolUse",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "postToolUse",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "postToolUseFailure",
        HookEventKind::State(HookBehavior::Error),
    ),
    HookEvent::new(
        "subagentStart",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "subagentStop",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "preCompact",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "afterAgentResponse",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "afterAgentThought",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new("stop", HookEventKind::Stop),
    HookEvent::new("sessionEnd", HookEventKind::SessionEnd),
];

impl HookProtocol for CursorProtocol {
    fn tool(&self) -> AiTool {
        AiTool::Cursor
    }
    fn name(&self) -> &'static str {
        "Cursor"
    }
    fn slug(&self) -> &'static str {
        "cursor"
    }
    fn config_filename(&self) -> &'static str {
        "hooks.json"
    }
    fn preview_filename(&self) -> &'static str {
        ".cursor/hooks.json"
    }
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    // Cursor 的 `stop` 事件通过 `status == "error"` 区分正常完成与异常结束，
    // 而不是像其他工具那样用独立的事件名表达错误。
    fn event_kind(&self, event: &HookEvent, status: Option<&str>) -> HookEventKind {
        if event.name == "stop" && status == Some("error") {
            return HookEventKind::State(HookBehavior::Error);
        }
        event.kind
    }

    // Cursor 的条目本身就是 `{ command }`，不像其他工具需要额外套一层
    // `{ hooks: [...] }`。
    fn handler(&self, _event: &HookEvent, commands: &ManagedCommands) -> Value {
        #[cfg(target_os = "windows")]
        let command = &commands.windows_powershell_host;
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let command = platform_command(commands);
        json!([{ "command": command }])
    }

    // Cursor 的配置文件根节点需要额外的 `version` 字段。
    fn config_root(&self, hooks: Map<String, Value>) -> Value {
        json!({ "version": 1, "hooks": Value::Object(hooks) })
    }

    // 条目结构比其他工具更扁平（没有嵌套的 `hooks` 数组），因此过滤逻辑
    // 直接对条目本身判断，而不是像默认实现那样先展开内层处理器列表。
    fn remove_managed_entries(&self, entries: &mut Vec<Value>) {
        entries.retain(|entry| !entry_is_managed(entry, self));
    }
}
