use super::{HookEventDecision, HookStateMachine, HookTransition};
use crate::domain::monitor::{AiTool, HookBehavior};

#[test]
fn resumed_session_keeps_old_turn_history() {
    let mut machine = HookStateMachine::default();
    machine.apply_event(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("session-1"),
        Some("turn-1"),
    );
    machine.apply_event(AiTool::Codex, "SessionEnd", Some("session-1"), None);

    assert_eq!(
        machine.apply_event(AiTool::Codex, "SessionStart", Some("session-1"), None),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(
        machine.apply_event(
            AiTool::Codex,
            "PreToolUse",
            Some("session-1"),
            Some("turn-1"),
        ),
        HookEventDecision::Ignore
    );
    assert_eq!(
        machine.apply_event(
            AiTool::Codex,
            "UserPromptSubmit",
            Some("session-1"),
            Some("turn-2"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
}

#[test]
fn codex_goal_progress_can_resume_after_overtaking_the_previous_stop() {
    let mut machine = HookStateMachine::default();
    machine.apply_event(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("session-1"),
        Some("turn-1"),
    );

    assert_eq!(
        machine.apply_event(
            AiTool::Codex,
            "PreToolUse",
            Some("session-1"),
            Some("turn-2"),
        ),
        HookEventDecision::Ignore
    );
    assert_eq!(
        machine.apply_event(AiTool::Codex, "Stop", Some("session-1"), Some("turn-1")),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(
        machine.apply_event(
            AiTool::Codex,
            "PreToolUse",
            Some("session-1"),
            Some("turn-2"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
}
