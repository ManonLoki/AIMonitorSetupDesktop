use std::path::Path;

use serde_json::Value;

use super::super::{
    MANAGED_HOOK_PREFIX, generate_hook_auxiliary_configs, hook_restart_required,
    managed_hook_marker, protocol, tool_from_slug,
};
use super::{command_has_marker, generate_hook_config};
use crate::domain::AppError;
use crate::domain::monitor::{AiTool, HookConfigPreview, merge_hook_config};

pub(super) fn generate_test_hook_config(tool: AiTool) -> Result<HookConfigPreview, AppError> {
    generate_hook_config(tool, Path::new("/opt/AIMonitor/AIMonitor"))
}

#[test]
fn every_generated_hook_contract_uses_canonical_slugs_events_and_markers() {
    for tool in AiTool::ALL {
        let protocol = protocol(tool);
        let slug = protocol.slug();
        let marker = managed_hook_marker(tool);
        let preview = generate_test_hook_config(tool).unwrap();

        assert_eq!(tool_from_slug(slug), Some(tool));
        assert_eq!(marker, format!("AIMonitor:tool={slug}"));
        assert!(preview.content.contains(&marker));
        assert!(!preview.content.contains("AIMonitor|tool="));
        for event in protocol.events() {
            assert!(
                preview.content.contains(event.name),
                "{} 的生成配置缺少协议事件 {}",
                protocol.name(),
                event.name
            );
        }

        if protocol.standalone_config().is_some() {
            assert!(preview.content.contains(&format!("/api/hooks/{slug}")));
            assert!(preview.content.contains("X-AIMonitor-Hook-Type"));
            assert!(preview.content.contains("hook_event_name"));
            assert!(preview.content.contains("session_id"));
            assert!(preview.content.contains("status"));
            assert!(
                generate_hook_auxiliary_configs(tool)
                    .iter()
                    .all(|file| file.content.contains(&marker))
            );
        } else {
            assert!(preview.content.contains("--aimonitor-hook-relay"));
            #[cfg(target_os = "windows")]
            if matches!(
                tool,
                AiTool::CodeBuddy | AiTool::WorkBuddy | AiTool::KimiCode
            ) {
                assert!(preview.content.contains(&format!("'{slug}'")));
            } else {
                assert!(preview.content.contains(slug));
            }
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            assert!(preview.content.contains(&format!("'{slug}'")));
            assert!(preview.content.contains("--managed-by"));
        }
    }
}

// 验证 Cursor 生成的 Hooks 配置：事件名使用 camelCase，包含预期事件，
// 不含 Claude/Codex 专属事件，且调用 AIMonitor 自身的轻量 relay 子命令。
#[test]
fn cursor_preview_uses_cursor_event_names_and_shape() {
    let preview = generate_test_hook_config(AiTool::Cursor).unwrap();

    assert_eq!(preview.filename, ".cursor/hooks.json");
    assert!(preview.content.contains("\"beforeSubmitPrompt\""));
    assert!(preview.content.contains("\"beforeShellExecution\""));
    assert!(preview.content.contains("\"beforeMCPExecution\""));
    assert!(preview.content.contains("\"afterFileEdit\""));
    assert!(preview.content.contains("\"workspaceOpen\""));
    assert!(preview.content.contains("\"postToolUse\""));
    assert!(preview.content.contains("\"subagentStart\""));
    assert!(preview.content.contains("\"subagentStop\""));
    assert!(preview.content.contains("\"preCompact\""));
    assert!(preview.content.contains("\"afterAgentResponse\""));
    assert!(preview.content.contains("\"sessionEnd\""));
    assert!(preview.content.contains("--aimonitor-hook-relay"));
    assert!(preview.content.contains("cursor"));
    assert!(preview.content.contains("AIMonitor:tool=cursor"));
    // 不应出现 Claude Code 专属的 Notification 事件（小写形式）。
    assert!(!preview.content.contains("\"notification\""));
    // Cursor 的条目结构没有 "type": "command" 字段（这是 Claude/Codex 的结构）。
    assert!(!preview.content.contains("\"type\": \"command\""));
}

// 验证 Claude Code 生成的 Hooks 配置：事件名使用 PascalCase，覆盖权限/生命周期事件，
// 且 Notification 事件带有 idle_prompt matcher，而普通事件（如 SessionStart）没有 matcher。
#[test]
fn claude_preview_covers_permission_and_lifecycle_events() {
    let preview = generate_test_hook_config(AiTool::ClaudeCode).unwrap();

    assert_eq!(preview.filename, ".claude/settings.json");
    assert!(preview.content.contains("\"SessionStart\""));
    assert!(preview.content.contains("\"PermissionRequest\""));
    assert!(preview.content.contains("\"Elicitation\""));
    assert!(preview.content.contains("\"PostToolUse\""));
    assert!(preview.content.contains("\"PostToolUseFailure\""));
    assert!(preview.content.contains("\"StopFailure\""));
    assert!(preview.content.contains("\"SessionEnd\""));
    assert!(preview.content.contains("\"Notification\""));
    assert!(preview.content.contains("AIMonitor:tool=claude-code"));
    assert!(preview.content.contains("--aimonitor-hook-relay"));
    let value: Value = serde_json::from_str(&preview.content).unwrap();
    // Notification 事件应带有 idle_prompt matcher，用于区分具体子类型。
    assert_eq!(value["hooks"]["Notification"][0]["matcher"], "idle_prompt");
    // 普通事件不应携带 matcher 字段。
    assert!(value["hooks"]["SessionStart"][0].get("matcher").is_none());
}

#[test]
fn every_wsl_command_hook_uses_its_posix_executable_and_config_path() {
    let cases = [
        (AiTool::Codex, ".codex/hooks.json"),
        (AiTool::ClaudeCode, ".claude/settings.json"),
        (AiTool::Cursor, ".cursor/hooks.json"),
        (AiTool::WorkBuddy, ".workbuddy/settings.json"),
        (AiTool::CodeBuddy, ".codebuddy/settings.json"),
    ];

    for (tool, expected_filename) in cases {
        let preview = super::generate_wsl_hook_config(
            tool,
            Path::new(r"C:\Program Files\AIMonitor\AIMonitor.exe"),
            "/mnt/c/Program Files/AIMonitor/AIMonitor.exe",
        )
        .unwrap();

        assert_eq!(preview.filename, expected_filename);
        assert!(
            preview
                .content
                .contains("'/mnt/c/Program Files/AIMonitor/AIMonitor.exe' --aimonitor-hook-relay")
        );
        assert!(!preview.content.contains("cmd.exe"));
        assert!(!preview.content.contains(r"C:\\Program Files"));
    }
}

// 验证 Codex 生成的 Hooks 配置：事件名使用 PascalCase 且没有独立 Error 事件；
// 每个 handler 只写入当前编译目标对应的命令，直接调用 AIMonitor 自身的
// 轻量 relay 子命令，不依赖 PowerShell/curl。
#[test]
fn codex_preview_uses_pascal_case_and_nested_handlers() {
    let preview = generate_test_hook_config(AiTool::Codex).unwrap();

    assert_eq!(preview.filename, ".codex/hooks.json");
    assert!(preview.content.contains("\"SessionStart\""));
    assert!(preview.content.contains("\"UserPromptSubmit\""));
    assert!(preview.content.contains("\"PermissionRequest\""));
    // Codex 没有独立的 "Error" 事件（异常状态通过其他事件间接体现）。
    assert!(!preview.content.contains("\"Error\""));
    assert!(preview.content.contains("\"PostToolUse\""));
    assert!(preview.content.contains("\"SessionEnd\""));
    assert!(preview.content.contains("AIMonitor:tool=codex"));
    assert!(preview.content.contains("\"type\": \"command\""));
    assert!(!preview.content.contains("\"commandWindows\""));
    assert!(!preview.content.contains("powershell.exe"));
    assert!(!preview.content.contains("Invoke-RestMethod"));
    assert!(preview.content.contains("/opt/AIMonitor/AIMonitor"));
    let value: Value = serde_json::from_str(&preview.content).unwrap();
    // 取出 SessionEnd 的当前平台命令，确认其中携带 relay 子命令和事件名。
    let session_end = value["hooks"]["SessionEnd"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(session_end.contains("--aimonitor-hook-relay"));
    assert!(session_end.contains("codex"));
    assert!(session_end.contains("SessionEnd"));
    assert_eq!(value["hooks"]["SessionEnd"][0]["hooks"][0]["timeout"], 3);
    let command = value["hooks"]["Stop"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(command.contains("--aimonitor-hook-relay"));
    assert!(command.contains(MANAGED_HOOK_PREFIX));
    // command_has_marker 应能在未解码的编码命令上直接识别出托管标记。
    assert!(command_has_marker(
        command,
        &managed_hook_marker(AiTool::Codex)
    ));
}

#[test]
#[cfg(target_os = "windows")]
fn windows_hook_command_quotes_the_installed_executable_without_powershell() {
    let preview = generate_hook_config(
        AiTool::Codex,
        Path::new(r"C:\Program Files\AIMonitor\AIMonitor.exe"),
    )
    .unwrap();
    let value: Value = serde_json::from_str(&preview.content).unwrap();
    let command = value["hooks"]["PostToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();

    assert!(command.starts_with("cmd.exe /d /s /c \"`\"C:\\Program Files"));
    assert!(command.contains("AIMonitor.exe`\" --aimonitor-hook-relay"));
    assert!(command.ends_with("--managed-by `\"AIMonitor:tool=codex`\"\""));
    assert!(!command.contains("powershell"));
    assert!(command_has_marker(
        command,
        &managed_hook_marker(AiTool::Codex)
    ));
}

#[test]
#[cfg(target_os = "windows")]
fn windows_command_variants_survive_a_powershell_host_without_creating_a_pipeline() {
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("aimonitor hook command {unique}"));
    fs::create_dir_all(&root).unwrap();
    let probe = root.join("argument probe.cmd");
    let output = root.join("arguments.txt");
    fs::write(
        &probe,
        format!("@echo off\r\n>\"{}\" echo %*\r\n", output.display()),
    )
    .unwrap();

    for tool in [AiTool::Codex, AiTool::Cursor] {
        let protocol = protocol(tool);
        let commands = super::managed_commands(protocol, protocol.events()[0].name, &probe, None);
        let command = &commands.windows_powershell_host;
        let result = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", command])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}: {}",
            protocol.name(),
            String::from_utf8_lossy(&result.stderr)
        );
        let arguments = fs::read_to_string(&output).unwrap();
        assert!(arguments.contains(&format!("AIMonitor:tool={}", protocol.slug())));
    }

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn work_buddy_preview_targets_its_independent_settings_file() {
    let preview = generate_test_hook_config(AiTool::WorkBuddy).unwrap();

    assert_eq!(preview.filename, ".workbuddy/settings.json");
    assert!(preview.content.contains("\"SessionStart\""));
    assert!(preview.content.contains("\"PermissionRequest\""));
    assert!(preview.content.contains("AIMonitor:tool=workbuddy"));
    let value: Value = serde_json::from_str(&preview.content).unwrap();
    let command = value["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(command.starts_with("'/opt/AIMonitor/AIMonitor'"));
    assert!(!command.contains("cmd.exe"));
    assert!(hook_restart_required(AiTool::WorkBuddy));
}

#[test]
fn open_code_preview_is_a_managed_global_plugin() {
    let preview = generate_test_hook_config(AiTool::OpenCode).unwrap();

    assert_eq!(preview.filename, ".config/opencode/plugins/aimonitor.js");
    assert!(preview.content.contains("AIMonitor:tool=opencode"));
    assert!(preview.content.contains("session.status"));
    assert!(preview.content.contains("permission.asked"));
    assert!(preview.content.contains("/api/hooks/opencode"));

    let merged = merge_hook_config(None, &preview, AiTool::OpenCode).unwrap();
    assert_eq!(merged.content, preview.content);
    let merged_again =
        merge_hook_config(Some(&merged.content), &preview, AiTool::OpenCode).unwrap();
    assert_eq!(merged_again.content, preview.content);
    assert!(
        merge_hook_config(
            Some("export const unrelated = true\n"),
            &preview,
            AiTool::OpenCode,
        )
        .is_err()
    );
}

#[test]
fn code_buddy_preview_uses_its_native_config_and_posix_hook_command() {
    let preview = generate_test_hook_config(AiTool::CodeBuddy).unwrap();

    assert_eq!(preview.filename, ".codebuddy/settings.json");
    assert!(preview.content.contains("\"PermissionRequest\""));
    assert!(preview.content.contains("AIMonitor:tool=codebuddy"));
    assert!(preview.content.contains("--aimonitor-hook-relay"));
    assert!(!preview.content.contains("powershell.exe"));
}

#[test]
fn hermes_preview_contains_a_complete_managed_plugin() {
    let preview = generate_test_hook_config(AiTool::Hermes).unwrap();
    let auxiliary = generate_hook_auxiliary_configs(AiTool::Hermes);

    assert_eq!(preview.filename, ".hermes/plugins/aimonitor/__init__.py");
    assert!(preview.content.contains("AIMonitor:tool=hermes"));
    assert!(preview.content.contains("ctx.register_hook"));
    assert!(preview.content.contains("pre_approval_request"));
    assert!(preview.content.contains("api_request_error"));
    assert!(preview.content.contains("/api/hooks/hermes"));
    assert_eq!(auxiliary.len(), 1);
    assert_eq!(auxiliary[0].filename, "plugins/aimonitor/plugin.yaml");
    assert!(auxiliary[0].content.contains("name: aimonitor"));

    let merged = merge_hook_config(None, &preview, AiTool::Hermes).unwrap();
    assert_eq!(merged.content, preview.content);
    assert!(
        merge_hook_config(
            Some("# unrelated Hermes plugin\n"),
            &preview,
            AiTool::Hermes,
        )
        .is_err()
    );
}

#[test]
fn open_claw_preview_contains_a_complete_managed_plugin() {
    let preview = generate_test_hook_config(AiTool::OpenClaw).unwrap();
    let auxiliary = generate_hook_auxiliary_configs(AiTool::OpenClaw);

    assert!(preview.content.contains("AIMonitor:tool=openclaw"));
    assert!(preview.content.contains("before_agent_run"));
    assert!(preview.content.contains("agent_end"));
    assert!(preview.content.contains("/api/hooks/openclaw"));
    assert_eq!(auxiliary.len(), 2);
    assert!(
        auxiliary
            .iter()
            .any(|file| file.filename.ends_with("openclaw.plugin.json"))
    );
    assert!(
        auxiliary
            .iter()
            .all(|file| file.content.contains("AIMonitor:tool=openclaw"))
    );
}
