use serde_json::Value;

use super::{
    HookEvent, HookEventKind, HookProtocol, ManagedCommands, command_group, platform_command,
};
use crate::domain::monitor::{AiTool, HookBehavior};

pub(super) static QWEN_CODE: QwenCodeProtocol = QwenCodeProtocol;

pub(super) struct QwenCodeProtocol;

// Qwen Code 的用户级 ~/.qwen/settings.json 使用 Claude-compatible command
// groups。只订阅能稳定推进四态展示的公开生命周期事件；流式 MessageDisplay
// 与 Todo 事件不会改变工作状态，因此不写入托管配置。
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
        "PostToolUseFailure",
        HookEventKind::State(HookBehavior::Error),
    ),
    HookEvent::new(
        "PermissionRequest",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new(
        "PermissionDenied",
        HookEventKind::State(HookBehavior::Error),
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
    // 同一个事件名在公共生成器中只能对应一个托管分组；权限询问已有
    // PermissionRequest，因此这里只保留 idle_prompt 的确定性空闲语义。
    HookEvent::with_matcher("Notification", "idle_prompt", HookEventKind::Stop),
    HookEvent::new("SessionEnd", HookEventKind::SessionEnd),
];

impl HookProtocol for QwenCodeProtocol {
    fn tool(&self) -> AiTool {
        AiTool::QwenCode
    }

    fn name(&self) -> &'static str {
        "Qwen Code"
    }

    fn slug(&self) -> &'static str {
        "qwen-code"
    }

    fn config_filename(&self) -> &'static str {
        "settings.json"
    }

    fn preview_filename(&self) -> &'static str {
        ".qwen/settings.json"
    }

    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        command_group(platform_command(commands), event.matcher)
    }

    // 新会话重新读取用户级设置，避免当前会话仍使用写入前的 Hook 快照。
    fn restart_required(&self) -> bool {
        true
    }
}
