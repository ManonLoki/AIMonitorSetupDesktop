use serde_json::Value;

use super::super::{hook_transition, managed_hook_marker};
use super::{command_has_marker, tests::generate_test_hook_config};
use crate::domain::monitor::{
    AiTool, DEFAULT_BASE_URL, HookBehavior, HookConfigPreview, HookTransition, merge_hook_config,
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
fn only_the_canonical_managed_marker_is_recognized() {
    assert!(command_has_marker(
        "'AIMonitor' --managed-by 'AIMonitor:tool=codex'",
        "AIMonitor:tool=codex"
    ));
    assert!(!command_has_marker(
        ": 'AIMonitor|tool=codex'; curl current",
        "AIMonitor:tool=codex"
    ));
}

// 验证 Cursor 的合并逻辑同样幂等，且能正确保留用户命令、去重已有的托管条目。
#[test]
fn cursor_merge_is_idempotent_and_preserves_other_commands() {
    let generated = generate_test_hook_config(AiTool::Cursor).unwrap();
    // 预置一份现有配置：包含用户命令和一条当前格式的托管条目。
    let existing = r#"{
      "version": 1,
      "hooks": {
        "stop": [
          { "command": "other-app stop" },
          { "command": "'AIMonitor' --aimonitor-hook-relay cursor stop --managed-by 'AIMonitor:tool=cursor'" }
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

#[test]
fn qwen_code_merge_is_idempotent_and_preserves_other_commands() {
    let generated = generate_test_hook_config(AiTool::QwenCode).unwrap();
    let existing = r#"{
      "hooks": {
        "SessionStart": [
          { "hooks": [{ "type": "command", "command": "other-session-start" }]}
        ],
        "Stop": [
          { "hooks": [{ "type": "command", "command": "other-stop" }]},
          { "hooks": [{ "type": "command", "command": "'AIMonitor' --aimonitor-hook-relay qwen-code Stop --managed-by 'AIMonitor:tool=qwen-code'" }]}
        ]
      }
    }"#;

    let first = merge_hook_config(Some(existing), &generated, AiTool::QwenCode).unwrap();
    let value: Value = serde_json::from_str(&first.content).unwrap();
    let stop = serde_json::to_string(&value["hooks"]["Stop"]).unwrap();

    assert_eq!(stop.matches("other-stop").count(), 1);
    assert_eq!(stop.matches("AIMonitor:tool=qwen-code").count(), 1);
    let second = merge_hook_config(Some(&first.content), &generated, AiTool::QwenCode).unwrap();
    assert_eq!(first.content, second.content);
}

#[test]
fn qoder_merge_is_idempotent_and_preserves_other_commands() {
    let generated = generate_test_hook_config(AiTool::Qoder).unwrap();
    let existing = r#"{
      "hooks": {
        "UserPromptSubmit": [
          { "hooks": [{ "type": "command", "command": "other-prompt" }]}
        ],
        "PostToolUse": [
          { "hooks": [{ "type": "command", "command": "'AIMonitor' --aimonitor-hook-relay qoder PostToolUse --managed-by 'AIMonitor:tool=qoder'" }]}
        ]
      }
    }"#;

    let first = merge_hook_config(Some(existing), &generated, AiTool::Qoder).unwrap();
    let value: Value = serde_json::from_str(&first.content).unwrap();
    let submit = serde_json::to_string(&value["hooks"]["UserPromptSubmit"]).unwrap();

    assert_eq!(submit.matches("other-prompt").count(), 1);
    assert_eq!(submit.matches("AIMonitor:tool=qoder").count(), 1);
    let second = merge_hook_config(Some(&first.content), &generated, AiTool::Qoder).unwrap();
    assert_eq!(first.content, second.content);
}

#[test]
fn gemini_cli_merge_is_idempotent_and_preserves_other_commands() {
    let generated = generate_test_hook_config(AiTool::GeminiCli).unwrap();
    let existing = r#"{
      "hooks": {
        "SessionStart": [
          { "hooks": [{ "type": "command", "command": "other-session-start" }]}
        ],
        "SessionEnd": [
          { "hooks": [{ "type": "command", "command": "other-session-end" }]}
        ]
      }
    }"#;

    let first = merge_hook_config(Some(existing), &generated, AiTool::GeminiCli).unwrap();
    let value: Value = serde_json::from_str(&first.content).unwrap();
    let session_end = serde_json::to_string(&value["hooks"]["SessionEnd"]).unwrap();

    assert_eq!(session_end.matches("other-session-end").count(), 1);
    assert_eq!(session_end.matches("AIMonitor:tool=gemini-cli").count(), 1);
    let second = merge_hook_config(Some(&first.content), &generated, AiTool::GeminiCli).unwrap();
    assert_eq!(first.content, second.content);
}

#[test]
fn github_copilot_merge_is_idempotent_and_preserves_other_handlers() {
    let generated = generate_test_hook_config(AiTool::GitHubCopilot).unwrap();
    let existing = r#"{
      "version": 99,
      "hooks": {
        "sessionStart": [
          { "type": "command", "command": "other-session-start" }
        ],
        "userPromptSubmitted": [
          { "type": "command", "command": "other-user-prompt" },
          {
            "type": "command",
            "command": "'AIMonitor' --aimonitor-hook-relay github-copilot userPromptSubmitted --managed-by 'AIMonitor:tool=github-copilot'"
          }
        ]
      }
    }"#;

    let first = merge_hook_config(Some(existing), &generated, AiTool::GitHubCopilot).unwrap();
    let value: Value = serde_json::from_str(&first.content).unwrap();
    let user_prompt = serde_json::to_string(&value["hooks"]["userPromptSubmitted"]).unwrap();

    assert_eq!(user_prompt.matches("other-user-prompt").count(), 1);
    assert_eq!(value["version"], 1);
    assert_eq!(
        user_prompt.matches("AIMonitor:tool=github-copilot").count(),
        1
    );
    let second =
        merge_hook_config(Some(&first.content), &generated, AiTool::GitHubCopilot).unwrap();
    assert_eq!(first.content, second.content);
}
