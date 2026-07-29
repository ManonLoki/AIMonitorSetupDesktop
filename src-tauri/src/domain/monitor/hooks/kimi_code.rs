use std::fmt::Write;

use serde_json::{Map, Value, json};

use super::{HookEvent, HookEventKind, HookProtocol, ManagedCommands, managed_hook_marker};
use crate::domain::monitor::{AiTool, HookBehavior, HookConfigPreview};

pub(super) static KIMI_CODE: KimiCodeProtocol = KimiCodeProtocol;

pub(super) struct KimiCodeProtocol;

// Kimi Code 的用户配置与 Hooks 共用 ~/.kimi-code/config.toml。只订阅能稳定
// 推进四态展示的公开生命周期事件；PermissionResult 与 Notification 不直接
// 对应确定状态，因此不写入托管规则。
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
    HookEvent::new("Stop", HookEventKind::Stop),
    HookEvent::new("StopFailure", HookEventKind::State(HookBehavior::Error)),
    HookEvent::new("Interrupt", HookEventKind::Stop),
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

impl HookProtocol for KimiCodeProtocol {
    fn tool(&self) -> AiTool {
        AiTool::KimiCode
    }

    fn name(&self) -> &'static str {
        "Kimi Code"
    }

    fn slug(&self) -> &'static str {
        "kimi-code"
    }

    fn config_filename(&self) -> &'static str {
        "config.toml"
    }

    fn preview_filename(&self) -> &'static str {
        ".kimi-code/config.toml"
    }

    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        // Kimi Code 在 Windows 上也明确使用 Git Bash 作为 shell，因此所有平台
        // 都写 POSIX 命令，不能落入公共 CMD 分支。
        let mut handler = json!({ "command": commands.posix });
        if let Some(matcher) = event.matcher {
            handler["matcher"] = Value::String(matcher.to_owned());
        }
        handler
    }

    fn render_config(&self, hooks: Map<String, Value>) -> Result<String, String> {
        let marker = managed_hook_marker(self.tool());
        let mut content = format!("# {marker} begin\n");

        for event in self.events() {
            let handler = hooks
                .get(event.name)
                .and_then(Value::as_object)
                .ok_or_else(|| format!("生成的 {} Hook 缺少处理器", event.name))?;
            let command = handler
                .get("command")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("生成的 {} Hook 缺少 command", event.name))?;

            content.push_str("[[hooks]]\n");
            writeln!(content, "event = {}", toml_string(event.name))
                .expect("writing to String cannot fail");
            if let Some(matcher) = handler.get("matcher").and_then(Value::as_str) {
                writeln!(content, "matcher = {}", toml_string(matcher))
                    .expect("writing to String cannot fail");
            }
            writeln!(content, "command = {}\n", toml_string(command))
                .expect("writing to String cannot fail");
        }

        writeln!(content, "# {marker} end").expect("writing to String cannot fail");
        Ok(content)
    }

    fn uses_custom_merge(&self) -> bool {
        true
    }

    // config.toml 同时保存模型、权限和用户自定义 Hook。只替换带完整边界标识的
    // AIMonitor 区块，其他 TOML 内容逐字保留；边界损坏时拒绝猜测性改写。
    fn merge_standalone(
        &self,
        existing_content: Option<&str>,
        generated: &HookConfigPreview,
    ) -> Result<String, String> {
        let mut merged = remove_managed_block(existing_content.unwrap_or(""), self.tool())?;
        if !merged.is_empty() {
            if !merged.ends_with('\n') {
                merged.push('\n');
            }
            if !merged.ends_with("\n\n") {
                merged.push('\n');
            }
        }
        merged.push_str(&generated.content);
        Ok(merged)
    }

    // Kimi Code 在新会话启动时读取 config.toml。
    fn restart_required(&self) -> bool {
        true
    }
}

fn remove_managed_block(content: &str, tool: AiTool) -> Result<String, String> {
    let marker = managed_hook_marker(tool);
    let begin = format!("# {marker} begin");
    let end = format!("# {marker} end");
    let begins = marker_line_ranges(content, &begin);
    let ends = marker_line_ranges(content, &end);

    if begins.is_empty() && ends.is_empty() {
        return Ok(content.to_owned());
    }
    if begins.len() != 1 || ends.len() != 1 || begins[0].0 >= ends[0].0 {
        return Err("现有 Kimi Code 配置中的 AIMonitor 托管区块边界不完整或重复".to_owned());
    }

    let mut stripped = String::with_capacity(content.len());
    stripped.push_str(&content[..begins[0].0]);
    stripped.push_str(&content[ends[0].1..]);
    Ok(stripped)
}

fn marker_line_ranges(content: &str, marker: &str) -> Vec<(usize, usize)> {
    let mut offset = 0;
    let mut ranges = Vec::new();
    for line in content.split_inclusive('\n') {
        let end = offset + line.len();
        if line.trim_end_matches(['\r', '\n']) == marker {
            ranges.push((offset, end));
        }
        offset = end;
    }
    ranges
}

fn toml_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                write!(escaped, "\\u{:04X}", character as u32)
                    .expect("writing to String cannot fail");
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}
