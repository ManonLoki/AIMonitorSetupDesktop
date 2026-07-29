use super::{merge_hook_config, tests::generate_test_hook_config};
use crate::domain::monitor::{
    AiTool, HookBehavior, HookEventDecision, HookStateMachine, HookTransition,
};

#[test]
fn kimi_code_preview_uses_managed_toml_rules() {
    let preview = generate_test_hook_config(AiTool::KimiCode).unwrap();

    assert_eq!(preview.filename, ".kimi-code/config.toml");
    assert!(preview.content.contains("AIMonitor:tool=kimi-code begin"));
    assert!(preview.content.contains("event = \"SessionStart\""));
    assert!(preview.content.contains("event = \"PermissionRequest\""));
    assert!(preview.content.contains("event = \"Interrupt\""));
    assert!(preview.content.contains("event = \"SessionEnd\""));
    assert_eq!(preview.content.matches("[[hooks]]").count(), 14);
    assert!(
        preview
            .content
            .contains("--aimonitor-hook-relay 'kimi-code'")
    );
    assert!(!preview.content.contains("cmd.exe"));
    assert!(super::super::hook_restart_required(AiTool::KimiCode));
}

#[test]
fn kimi_code_merge_preserves_user_toml_and_is_idempotent() {
    let generated = generate_test_hook_config(AiTool::KimiCode).unwrap();
    let existing = "default_model = \"kimi-code/k3\"\n\n[background]\nkeep_alive_on_exit = true\n";

    let first = merge_hook_config(Some(existing), &generated, AiTool::KimiCode).unwrap();
    assert!(first.content.starts_with(existing));
    assert!(first.content.contains("default_model = \"kimi-code/k3\""));
    assert_eq!(
        first
            .content
            .matches("AIMonitor:tool=kimi-code begin")
            .count(),
        1
    );

    let second = merge_hook_config(Some(&first.content), &generated, AiTool::KimiCode).unwrap();
    assert_eq!(first.content, second.content);
}

#[test]
fn kimi_code_merge_rejects_a_broken_managed_block() {
    let generated = generate_test_hook_config(AiTool::KimiCode).unwrap();
    let broken = "default_model = \"kimi-code/k3\"\n# AIMonitor:tool=kimi-code begin\n";

    assert!(merge_hook_config(Some(broken), &generated, AiTool::KimiCode).is_err());
}

#[test]
fn kimi_code_state_machine_covers_permission_failure_stop_and_release() {
    let mut machine = HookStateMachine::default();
    assert_eq!(
        machine.apply_event(AiTool::KimiCode, "SessionStart", Some("kimi"), None),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(
        machine.apply_event(AiTool::KimiCode, "UserPromptSubmit", Some("kimi"), None,),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    assert_eq!(
        machine.apply_event(AiTool::KimiCode, "PermissionRequest", Some("kimi"), None,),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking))
    );
    assert_eq!(
        machine.apply_event(AiTool::KimiCode, "PostToolUseFailure", Some("kimi"), None,),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
    );
    assert_eq!(
        machine.apply_event(AiTool::KimiCode, "Interrupt", Some("kimi"), None),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(
        machine.apply_event(AiTool::KimiCode, "SessionEnd", Some("kimi"), None),
        HookEventDecision::Forward(HookTransition::Release)
    );
}
