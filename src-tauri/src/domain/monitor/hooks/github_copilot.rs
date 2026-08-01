use serde_json::{Map, Value, json};

use super::{
    HookEvent, HookEventKind, HookProtocol, ManagedCommands, entry_is_managed, platform_command,
};
use crate::domain::monitor::{AiTool, HookBehavior, HookWriteOutcome};

pub(super) static GITHUB_COPILOT: GitHubCopilotProtocol = GitHubCopilotProtocol;

pub(super) struct GitHubCopilotProtocol;

// Copilot CLI 用户级 Hook 文件采用官方 lower-camel 事件名与扁平 handler 数组。
// userPromptTransformed 只负责改写模型输入，不表达新的生命周期状态；notification
// 的原生 hook_event_name 与配置键大小写不同，当前也已有 permissionRequest 表达询问态，
// 因此两者都不进入本轮托管事件表。
const EVENTS: &[HookEvent] = &[
    HookEvent::new("sessionStart", HookEventKind::SessionStart),
    HookEvent::new("userPromptSubmitted", HookEventKind::WorkStart),
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
        "permissionRequest",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new("agentStop", HookEventKind::Stop),
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
    HookEvent::new("errorOccurred", HookEventKind::State(HookBehavior::Error)),
    HookEvent::new("sessionEnd", HookEventKind::SessionEnd),
];

impl HookProtocol for GitHubCopilotProtocol {
    fn tool(&self) -> AiTool {
        AiTool::GitHubCopilot
    }

    fn name(&self) -> &'static str {
        "GitHub Copilot CLI"
    }

    fn slug(&self) -> &'static str {
        "github-copilot"
    }

    // application 层把用户选择的 ~/.copilot 与该相对路径组合成独立的
    // ~/.copilot/hooks/aimonitor.json 文件。
    fn config_filename(&self) -> &'static str {
        "hooks/aimonitor.json"
    }

    fn preview_filename(&self) -> &'static str {
        ".copilot/hooks/aimonitor.json"
    }

    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        // Copilot 的 `command` 会回退到平台对应的 bash/powershell 字段；Windows
        // 因而需要能先经过 PowerShell 再交给 CMD 的引号形式。WSL 仍使用 POSIX。
        #[cfg(target_os = "windows")]
        let command = if commands.is_wsl {
            platform_command(commands)
        } else {
            &commands.windows_powershell_host
        };
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let command = platform_command(commands);
        let mut handler = json!({
            "type": "command",
            "command": command,
        });
        if let Some(matcher) = event.matcher {
            handler["matcher"] = Value::String(matcher.to_owned());
        }
        Value::Array(vec![handler])
    }

    fn config_root(&self, hooks: Map<String, Value>) -> Value {
        json!({ "version": 1, "hooks": Value::Object(hooks) })
    }

    // Copilot 的事件值直接是 handler 数组，不带 Claude-style `{ hooks: [...] }`
    // 分组，因此按扁平条目上的托管命令标识清理旧规则。
    fn remove_managed_entries(&self, entries: &mut Vec<Value>) {
        entries.retain(|entry| !entry_is_managed(entry, self));
    }

    // Copilot CLI 启动时加载用户级 Hook 文件，写入后需重启或新建会话。
    fn changed_write_outcome(&self) -> HookWriteOutcome {
        HookWriteOutcome::RestartRequired
    }
}
