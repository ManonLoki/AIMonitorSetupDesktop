use serde_json::{Value, json};

use super::{HookEvent, HookEventKind, HookProtocol, managed_hook_marker, shell_quote};
use crate::domain::monitor::{AiTool, DEFAULT_HOOK_RELAY_PORT, HookBehavior, HookConfigPreview};

pub(super) static HARNESS: HarnessProtocol = HarnessProtocol;

pub(super) struct HarnessProtocol;

const EVENTS: &[HookEvent] = &[HookEvent::new(
    "agent-state-changed",
    HookEventKind::State(HookBehavior::Idle),
)];

impl HookProtocol for HarnessProtocol {
    fn tool(&self) -> AiTool {
        AiTool::Harness
    }

    fn name(&self) -> &'static str {
        "Harness"
    }

    fn slug(&self) -> &'static str {
        "harness"
    }

    fn config_filename(&self) -> &'static str {
        "hooks.json"
    }

    fn preview_filename(&self) -> &'static str {
        "Library/Application Support/Harness/hooks.json"
    }

    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    fn event_kind(&self, event: &HookEvent, status: Option<&str>) -> HookEventKind {
        match status {
            Some("working") => HookEventKind::State(HookBehavior::Running),
            Some("awaiting") => HookEventKind::State(HookBehavior::Asking),
            Some("errored") => HookEventKind::State(HookBehavior::Error),
            _ => event.kind,
        }
    }

    // Harness 是常驻守护进程，需要重启才能重新加载 hooks.json。
    fn restart_required(&self) -> bool {
        true
    }

    fn standalone_config(&self) -> Option<String> {
        let marker = managed_hook_marker(AiTool::Harness);
        let endpoint = format!("http://127.0.0.1:{DEFAULT_HOOK_RELAY_PORT}/api/hooks/harness");
        // Harness 的 run-shell Hook 不展开 FormatString，因此在事件发生时通过官方
        // list-agents JSON 接口计算全局优先级：询问 > 异常 > 运行 > 空闲。
        let command = format!(
            ": {}; harness_cli=\"$(command -v harness-cli 2>/dev/null || true)\"; \
             if [ -z \"$harness_cli\" ]; then if [ -n \"${{HARNESS_HOME:-}}\" ]; then harness_cli=\"$HARNESS_HOME/bin/harness-cli\"; \
             elif [ \"$(uname -s)\" = Darwin ]; then harness_cli=\"$HOME/Library/Application Support/Harness/bin/harness-cli\"; \
             else harness_cli=\"${{XDG_DATA_HOME:-$HOME/.local/share}}/harness/bin/harness-cli\"; fi; fi; \
             [ -x \"$harness_cli\" ] || exit 0; agents=\"$(\"$harness_cli\" list-agents --json 2>/dev/null)\" || exit 0; \
             status=idle; if printf '%s' \"$agents\" | grep -q '\"activity\"[[:space:]]*:[[:space:]]*\"awaiting\"'; then status=awaiting; \
             elif printf '%s' \"$agents\" | grep -q '\"activity\"[[:space:]]*:[[:space:]]*\"errored\"'; then status=errored; \
             elif printf '%s' \"$agents\" | grep -q '\"activity\"[[:space:]]*:[[:space:]]*\"working\"'; then status=working; fi; \
             printf '{{\"hook_event_name\":\"agent-state-changed\",\"status\":\"%s\"}}' \"$status\" | \
             curl --silent --show-error --fail --connect-timeout 1 --max-time 3 --request POST \
             --header 'Content-Type: application/json' --header 'X-AIMonitor-Hook-Type: agent-state-changed' \
             --data-binary @- {} >/dev/null",
            shell_quote(&marker),
            shell_quote(&endpoint),
        );
        let managed = json!([{
            "id": "8f4e58c1-a7ab-4ec8-96b4-8f8eab06a154",
            "event": "agent-state-changed",
            "command": {
                "runShell": {
                    "shellCommand": command,
                    "captureToBuffer": false
                }
            },
            "conditionFormat": null
        }]);
        serde_json::to_string_pretty(&managed).ok()
    }

    fn merge_standalone(
        &self,
        existing_content: Option<&str>,
        generated: &HookConfigPreview,
    ) -> Result<String, String> {
        let mut existing = match existing_content {
            Some(content) => serde_json::from_str::<Vec<Value>>(content)
                .map_err(|error| format!("现有 Harness Hooks 配置格式错误：{error}"))?,
            None => Vec::new(),
        };
        let marker = managed_hook_marker(AiTool::Harness);
        existing.retain(|entry| !entry.to_string().contains(&marker));
        let generated_entries = serde_json::from_str::<Vec<Value>>(&generated.content)
            .map_err(|error| format!("生成的 Harness Hooks 配置格式错误：{error}"))?;
        existing.extend(generated_entries);
        serde_json::to_string_pretty(&existing)
            .map_err(|error| format!("无法合并 Harness Hooks 配置：{error}"))
    }
}
