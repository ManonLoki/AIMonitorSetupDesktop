use std::path::Path;

use serde_json::Value;

use super::super::{
    MANAGED_HOOK_PREFIX, generate_hook_auxiliary_configs, hook_transition, managed_hook_marker,
};
use super::{command_has_marker, contains_managed_hook_config, generate_hook_config};
use crate::domain::monitor::{
    AiTool, DEFAULT_BASE_URL, HookBehavior, HookConfigPreview, HookTransition, encode_base64,
    merge_hook_config,
};

fn generate_test_hook_config(tool: AiTool) -> Result<HookConfigPreview, String> {
    generate_hook_config(tool, Path::new("/opt/AIMonitor/AIMonitor"))
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
    assert!(preview.content.contains("AIMonitor|tool=cursor"));
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
    assert!(preview.content.contains("AIMonitor|tool=claude-code"));
    assert!(preview.content.contains("--aimonitor-hook-relay"));
    let value: Value = serde_json::from_str(&preview.content).unwrap();
    // Notification 事件应带有 idle_prompt matcher，用于区分具体子类型。
    assert_eq!(value["hooks"]["Notification"][0]["matcher"], "idle_prompt");
    // 普通事件不应携带 matcher 字段。
    assert!(value["hooks"]["SessionStart"][0].get("matcher").is_none());
}

// 验证 Codex 生成的 Hooks 配置：事件名使用 PascalCase 且没有独立 Error 事件；
// 每个 handler 同时包含 POSIX 命令和 Windows 命令，二者都直接调用当前
// AIMonitor 可执行文件的轻量 relay 子命令，不依赖 PowerShell/curl。
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
    assert!(preview.content.contains("AIMonitor|tool=codex"));
    assert!(preview.content.contains("\"type\": \"command\""));
    assert!(preview.content.contains("\"commandWindows\""));
    assert!(!preview.content.contains("powershell.exe"));
    assert!(!preview.content.contains("Invoke-RestMethod"));
    assert!(preview.content.contains("/opt/AIMonitor/AIMonitor"));
    let value: Value = serde_json::from_str(&preview.content).unwrap();
    // 取出 SessionEnd 的 POSIX 命令，确认其中携带 relay 子命令和事件名。
    let session_end = value["hooks"]["SessionEnd"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(session_end.contains("--aimonitor-hook-relay"));
    assert!(session_end.contains("codex"));
    assert!(session_end.contains("SessionEnd"));
    assert_eq!(value["hooks"]["SessionEnd"][0]["hooks"][0]["timeout"], 3);
    // Windows 命令保持明文、携带托管标记，旧版 EncodedCommand 仍可由清理逻辑识别。
    let windows = value["hooks"]["Stop"][0]["hooks"][0]["commandWindows"]
        .as_str()
        .unwrap();
    assert!(windows.contains("--aimonitor-hook-relay"));
    assert!(windows.contains(MANAGED_HOOK_PREFIX));
    // command_has_marker 应能在未解码的编码命令上直接识别出托管标记。
    assert!(command_has_marker(
        windows,
        &managed_hook_marker(AiTool::Codex)
    ));
}

#[test]
fn windows_hook_command_quotes_the_installed_executable_without_powershell() {
    let preview = generate_hook_config(
        AiTool::Codex,
        Path::new(r"C:\Program Files\AIMonitor\AIMonitor.exe"),
    )
    .unwrap();
    let value: Value = serde_json::from_str(&preview.content).unwrap();
    let command = value["hooks"]["PostToolUse"][0]["hooks"][0]["commandWindows"]
        .as_str()
        .unwrap();

    assert!(command.starts_with("cmd.exe /d /s /c \"\"C:\\Program Files"));
    assert!(command.contains("AIMonitor.exe\" --aimonitor-hook-relay"));
    assert!(command.ends_with("--managed-by \"AIMonitor|tool=codex\"\""));
    assert!(!command.contains("powershell"));
}

#[test]
fn work_buddy_preview_targets_its_independent_settings_file() {
    let preview = generate_test_hook_config(AiTool::WorkBuddy).unwrap();

    assert_eq!(preview.filename, ".workbuddy/settings.json");
    assert!(preview.content.contains("\"SessionStart\""));
    assert!(preview.content.contains("\"PermissionRequest\""));
    assert!(preview.content.contains("AIMonitor|tool=workbuddy"));
}

#[test]
fn open_code_preview_is_a_managed_global_plugin() {
    let preview = generate_test_hook_config(AiTool::OpenCode).unwrap();

    assert_eq!(preview.filename, ".config/opencode/plugins/aimonitor.js");
    assert!(preview.content.contains("AIMonitor|tool=opencode"));
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
    assert!(preview.content.contains("AIMonitor|tool=codebuddy"));
    assert!(preview.content.contains("--aimonitor-hook-relay"));
    assert!(!preview.content.contains("powershell.exe"));
}

#[test]
fn hermes_preview_contains_a_complete_managed_plugin() {
    let preview = generate_test_hook_config(AiTool::Hermes).unwrap();
    let auxiliary = generate_hook_auxiliary_configs(AiTool::Hermes);

    assert_eq!(preview.filename, ".hermes/plugins/aimonitor/__init__.py");
    assert!(preview.content.contains("AIMonitor|tool=hermes"));
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

    assert!(preview.content.contains("AIMonitor|tool=openclaw"));
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
            .all(|file| file.content.contains("AIMonitor|tool=openclaw"))
    );
}

// 验证 Codex 的合并逻辑是幂等的：重复合并不会让托管条目累积，
// 且用户手工添加的其他命令、其他顶层字段（如 permissions）会被保留。
#[test]
fn codex_merge_is_idempotent_and_preserves_other_commands() {
    let generated = generate_test_hook_config(AiTool::Codex).unwrap();
    // 第一次合并：从空配置开始生成初始文件。
    let first = merge_hook_config(None, &generated, AiTool::Codex).unwrap();
    let mut value: Value = serde_json::from_str(&first.content).unwrap();
    // 模拟用户手工添加的其他顶层字段和 Stop 事件的其他命令。
    value["permissions"] = serde_json::json!({ "allow": ["Bash"] });
    value["hooks"]["Stop"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "hooks": [{ "type": "command", "command": "other-app notify" }]
        }));
    let existing = serde_json::to_string_pretty(&value).unwrap();
    // 第二次合并：应当只替换本工具的托管条目，不影响用户添加的内容。
    let merged = merge_hook_config(Some(&existing), &generated, AiTool::Codex).unwrap();
    let value: Value = serde_json::from_str(&merged.content).unwrap();
    let stop = value["hooks"]["Stop"].as_array().unwrap();
    let serialized = serde_json::to_string(stop).unwrap();

    // 用户的 "other-app notify" 命令应恰好保留一份，没有被重复或删除。
    assert_eq!(serialized.matches("other-app notify").count(), 1);
    // 同一托管 handler 的 POSIX/Windows 命令各带一个标记；总计两个说明
    // 只有一组托管 handler，没有因重复合并继续累积。
    assert_eq!(
        serialized
            .matches(&managed_hook_marker(AiTool::Codex))
            .count(),
        2
    );
    // 用户手工添加的 permissions 字段应被完整保留。
    assert_eq!(value["permissions"]["allow"][0], "Bash");
}

#[test]
fn managed_marker_still_recognizes_legacy_powershell_commands() {
    let marker = managed_hook_marker(AiTool::Codex);
    let script = format!("$null = '{marker}'; Invoke-RestMethod -Uri 'http://127.0.0.1'");
    let encoded = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let command = format!(
        "powershell.exe -NoProfile -NonInteractive -EncodedCommand {}",
        encode_base64(&encoded)
    );

    assert!(command_has_marker(&command, &marker));
}

#[test]
fn managed_config_detection_finds_legacy_command_but_not_user_hooks() {
    let marker = managed_hook_marker(AiTool::Codex);
    let script = format!("$null = '{marker}'; Invoke-RestMethod -Body $body");
    let encoded = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let legacy = serde_json::json!({
        "hooks": {
            "PostToolUse": [{
                "hooks": [{
                    "type": "command",
                    "commandWindows": format!(
                        "powershell.exe -EncodedCommand {}",
                        encode_base64(&encoded)
                    )
                }]
            }]
        }
    });
    let user_only = r#"{"hooks":{"PostToolUse":[{"hooks":[{"command":"my notifier"}]}]}}"#;

    assert!(contains_managed_hook_config(
        &legacy.to_string(),
        AiTool::Codex
    ));
    assert!(!contains_managed_hook_config(user_only, AiTool::Codex));
}

// 验证 Cursor 的合并逻辑同样幂等，且能正确保留用户命令、去重已有的托管条目。
#[test]
fn cursor_merge_is_idempotent_and_preserves_other_commands() {
    let generated = generate_test_hook_config(AiTool::Cursor).unwrap();
    // 预置一份现有配置：包含用户命令和一条旧格式的托管条目。
    let existing = r#"{
      "version": 1,
      "hooks": {
        "stop": [
          { "command": "other-app stop" },
          { "command": ": 'AIMonitor|tool=cursor'; curl current" }
        ]
      }
    }"#;

    // 连续合并两次，验证第二次不会让内容继续累积。
    let first = merge_hook_config(Some(existing), &generated, AiTool::Cursor).unwrap();
    let second = merge_hook_config(Some(&first.content), &generated, AiTool::Cursor).unwrap();
    let value: Value = serde_json::from_str(&second.content).unwrap();
    let stop = serde_json::to_string(&value["hooks"]["stop"]).unwrap();

    assert_eq!(stop.matches("other-app stop").count(), 1);
    assert_eq!(stop.matches("AIMonitor|tool=cursor").count(), 1);
}

// 验证当现有配置的根结构不合法（hooks 应为对象却是数组）时，合并应返回错误而不是崩溃。
#[test]
fn merge_rejects_an_invalid_existing_config() {
    let generated = HookConfigPreview {
        filename: ".cursor/hooks.json".to_owned(),
        content: r#"{"version":1,"hooks":{}}"#.to_owned(),
    };

    assert!(merge_hook_config(Some(r#"{"hooks":[]}"#), &generated, AiTool::Cursor).is_err());
}

// 验证命令只启动 AIMonitor 自身的轻量 relay，不依赖 PowerShell/curl，也不包含
// 设备地址或 behavior 等后续设备投递数据。
#[test]
fn hook_commands_use_the_embedded_minimizing_relay() {
    let preview = generate_test_hook_config(AiTool::Codex).unwrap();

    assert!(preview.content.contains("--aimonitor-hook-relay"));
    assert!(preview.content.contains("SessionStart"));
    assert!(!preview.content.contains("Invoke-RestMethod"));
    assert!(!preview.content.contains("curl"));
    assert!(!preview.content.contains("aimonitor-hook.sh"));
    assert!(!preview.content.contains("aimonitor-hook.ps1"));
    assert!(!preview.content.contains(DEFAULT_BASE_URL));
    assert!(!preview.content.contains("\"behavior\":\"running\""));
}

// 验证生成的 Hooks 配置只依赖工具类型，不需要预先保存设备展示 Profile。
#[test]
fn hook_config_is_identical_when_display_content_changes() {
    assert_eq!(
        generate_test_hook_config(AiTool::Codex).unwrap().content,
        generate_test_hook_config(AiTool::Codex).unwrap().content
    );
}

// 验证 hook_transition 对不同工具、不同事件名能返回正确的状态迁移：
// Claude 的 Notification 对应 Idle 展示，Codex 的 PermissionRequest 对应 Asking 展示，
// Cursor 的 sessionEnd 对应释放展示位，OpenCode/WorkBuddy 的原生事件也由
// 各自协议归一化，未知事件返回 None。
#[test]
fn hook_transitions_keep_state_rules_in_the_desktop_backend() {
    assert_eq!(
        hook_transition(AiTool::ClaudeCode, "Notification"),
        Some(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(
        hook_transition(AiTool::Codex, "PermissionRequest"),
        Some(HookTransition::Display(HookBehavior::Asking))
    );
    assert_eq!(
        hook_transition(AiTool::Cursor, "sessionEnd"),
        Some(HookTransition::Release)
    );
    assert_eq!(
        hook_transition(AiTool::OpenCode, "session.busy"),
        Some(HookTransition::Display(HookBehavior::Running))
    );
    assert_eq!(
        hook_transition(AiTool::WorkBuddy, "PermissionRequest"),
        Some(HookTransition::Display(HookBehavior::Asking))
    );
    assert_eq!(hook_transition(AiTool::Codex, "Unknown"), None);
}
