use serde_json::Value;

use super::super::{hook_restart_required, managed_hook_marker};
use super::{command_has_marker, tests::generate_test_hook_config};
use crate::domain::monitor::AiTool;

#[test]
fn qwen_code_preview_matches_official_contract() {
    let preview = generate_test_hook_config(AiTool::QwenCode).unwrap();
    let marker = managed_hook_marker(AiTool::QwenCode);
    let value: Value = serde_json::from_str(&preview.content).unwrap();

    assert_eq!(preview.filename, ".qwen/settings.json");
    assert!(hook_restart_required(AiTool::QwenCode));
    assert!(preview.content.contains(&marker));
    assert!(preview.content.contains("SessionStart"));
    assert!(preview.content.contains("SessionEnd"));
    assert!(preview.content.contains("PermissionRequest"));
    assert!(preview.content.contains("PermissionDenied"));
    assert!(preview.content.contains("Notification"));

    let hooks = value["hooks"].as_object().unwrap();
    assert!(hooks.contains_key("SessionStart"));
    assert!(hooks.contains_key("PreCompact"));
    assert!(hooks.contains_key("PostCompact"));
    assert!(hooks.contains_key("Notification"));
    assert!(hooks.contains_key("SessionEnd"));
    assert!(!hooks.contains_key("version"));

    let user_prompt = hooks["UserPromptSubmit"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(command_has_marker(user_prompt, &marker));
    assert_eq!(hooks["Notification"][0]["matcher"], "idle_prompt");
}

#[test]
fn qoder_preview_is_user_settings_file_and_hot_reloads() {
    let preview = generate_test_hook_config(AiTool::Qoder).unwrap();
    let marker = managed_hook_marker(AiTool::Qoder);
    let value: Value = serde_json::from_str(&preview.content).unwrap();

    assert_eq!(preview.filename, ".qoder/settings.json");
    assert!(!hook_restart_required(AiTool::Qoder));
    assert!(preview.content.contains(&marker));
    let hooks = value["hooks"].as_object().unwrap();
    assert!(hooks.contains_key("UserPromptSubmit"));
    assert!(hooks.contains_key("PreToolUse"));
    assert!(hooks.contains_key("PostToolUse"));
    assert!(hooks.contains_key("PostToolUseFailure"));
    assert!(hooks.contains_key("Stop"));

    let stop = hooks["Stop"][0]["hooks"][0]["command"].as_str().unwrap();
    assert!(command_has_marker(stop, &marker));
    assert!(stop.contains("qoder"));
    assert!(stop.contains("Stop"));
}

#[test]
fn gemini_cli_preview_uses_expected_event_matchers() {
    let preview = generate_test_hook_config(AiTool::GeminiCli).unwrap();
    let marker = managed_hook_marker(AiTool::GeminiCli);
    let value: Value = serde_json::from_str(&preview.content).unwrap();

    assert_eq!(preview.filename, ".gemini/settings.json");
    assert!(hook_restart_required(AiTool::GeminiCli));
    assert!(preview.content.contains(&marker));
    assert!(preview.content.contains("\"BeforeAgent\""));
    assert!(preview.content.contains("\"Notification\""));

    let hooks = value["hooks"].as_object().unwrap();
    assert!(hooks.contains_key("BeforeAgent"));
    assert!(hooks.contains_key("AfterAgent"));
    assert!(hooks.contains_key("SessionEnd"));
    assert_eq!(hooks["Notification"][0]["matcher"], "ToolPermission");
}

#[test]
fn github_copilot_preview_is_flat_version_one_contract() {
    let preview = generate_test_hook_config(AiTool::GitHubCopilot).unwrap();
    let marker = managed_hook_marker(AiTool::GitHubCopilot);
    let value: Value = serde_json::from_str(&preview.content).unwrap();

    assert_eq!(preview.filename, ".copilot/hooks/aimonitor.json");
    assert!(hook_restart_required(AiTool::GitHubCopilot));
    assert_eq!(value["version"], 1);
    assert!(preview.content.contains(&marker));
    let hooks = value["hooks"].as_object().unwrap();
    assert!(hooks.contains_key("userPromptSubmitted"));
    assert!(hooks.contains_key("permissionRequest"));
    assert!(hooks.contains_key("sessionEnd"));
    assert!(hooks["sessionStart"][0].get("hooks").is_none());

    let user_prompt = hooks["userPromptSubmitted"][0]["command"].as_str().unwrap();
    assert!(command_has_marker(user_prompt, &marker));
    assert_eq!(hooks["sessionStart"][0]["type"], "command");
}
