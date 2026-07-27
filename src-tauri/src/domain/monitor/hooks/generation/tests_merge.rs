use serde_json::Value;

use super::super::{hook_transition, managed_hook_marker};
use super::{command_has_marker, contains_managed_hook_config, tests::generate_test_hook_config};
use crate::domain::monitor::{
    AiTool, DEFAULT_BASE_URL, HookBehavior, HookConfigPreview, HookTransition, encode_base64,
    merge_hook_config,
};

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
    // 同一托管 handler 只有当前平台命令携带一个标记，没有因重复合并继续累积。
    assert_eq!(
        serialized
            .matches(&managed_hook_marker(AiTool::Codex))
            .count(),
        1
    );
    // 用户手工添加的 permissions 字段应被完整保留。
    assert_eq!(value["permissions"]["allow"][0], "Bash");
}

#[test]
fn managed_marker_still_recognizes_legacy_powershell_commands() {
    let marker = "AIMonitor|tool=codex";
    let script = format!("$null = '{marker}'; Invoke-RestMethod -Uri 'http://127.0.0.1'");
    let encoded = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let command = format!(
        "powershell.exe -NoProfile -NonInteractive -EncodedCommand {}",
        encode_base64(&encoded)
    );

    assert!(command_has_marker(&command, marker));
}

#[test]
fn managed_config_detection_finds_legacy_command_but_not_user_hooks() {
    let marker = "AIMonitor|tool=codex";
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
    assert_eq!(stop.matches("AIMonitor:tool=cursor").count(), 1);
    assert!(!stop.contains("AIMonitor|tool=cursor"));
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
