use serde_json::Value;

use super::{
    HookEvent, HookEventKind, HookProtocol, ManagedCommands, command_group, platform_command,
};
use crate::domain::monitor::{AiTool, HookBehavior};

pub(super) static QODER: QoderProtocol = QoderProtocol;

pub(super) struct QoderProtocol;

// Qoder IDE、JetBrains 插件与 CLI 共用 ~/.qoder/settings.json。这里采用三种
// 入口都公开的五事件兼容基线；CLI 独有扩展事件不能写入共享用户配置。
const EVENTS: &[HookEvent] = &[
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
    HookEvent::new("Stop", HookEventKind::Stop),
];

impl HookProtocol for QoderProtocol {
    fn tool(&self) -> AiTool {
        AiTool::Qoder
    }

    fn name(&self) -> &'static str {
        "Qoder"
    }

    fn slug(&self) -> &'static str {
        "qoder"
    }

    fn config_filename(&self) -> &'static str {
        "settings.json"
    }

    fn preview_filename(&self) -> &'static str {
        ".qoder/settings.json"
    }

    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        command_group(platform_command(commands), event.matcher)
    }

    // Qoder 会立即加载 settings.json 的 Hook 变更，沿用 trait 的无需重启默认值。
}
