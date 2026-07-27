use serde_json::{Value, json};

#[cfg(any(target_os = "macos", target_os = "linux"))]
use super::platform_command;
use super::{HookEvent, HookEventKind, HookProtocol, ManagedCommands};
use crate::domain::monitor::{AiTool, HookBehavior};

pub(super) static CODEX: CodexProtocol = CodexProtocol;

pub(super) struct CodexProtocol;

const EVENTS: &[HookEvent] = &[
    HookEvent::new("SessionStart", HookEventKind::SessionStart),
    HookEvent::new("UserPromptSubmit", HookEventKind::WorkStart),
    HookEvent::new(
        "PreToolUse",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "PostToolUse",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "PermissionRequest",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new("Stop", HookEventKind::Stop),
    HookEvent::new(
        "SubagentStart",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "SubagentStop",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "PreCompact",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "PostCompact",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new("SessionEnd", HookEventKind::SessionEnd),
];

impl HookProtocol for CodexProtocol {
    fn tool(&self) -> AiTool {
        AiTool::Codex
    }
    fn name(&self) -> &'static str {
        "Codex"
    }
    fn slug(&self) -> &'static str {
        "codex"
    }
    fn config_filename(&self) -> &'static str {
        "hooks.json"
    }
    fn preview_filename(&self) -> &'static str {
        ".codex/hooks.json"
    }
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        #[cfg(target_os = "windows")]
        let command = &commands.windows_powershell_host;
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let command = platform_command(commands);
        let mut command = json!({
            "type": "command",
            "command": command,
        });
        if event.name == "SessionEnd" {
            command["timeout"] = json!(3);
        }
        json!([{ "hooks": [command] }])
    }

    // Codex 需要在 CLI 中运行 /hooks 审核并信任新增规则。
    fn requires_review(&self) -> bool {
        true
    }

    // Codex 启动时会快照 Hooks 配置，需重启 App 或新建任务才能生效。
    fn restart_required(&self) -> bool {
        true
    }
}
