use std::time::Duration;

use super::{HookEventDecision, HookPhase, HookStateMachine, HookTransition};
use crate::domain::monitor::{AiTool, HookBehavior};

#[test]
fn cursor_tombstone_rejects_late_start_but_accepts_a_new_generation() {
    let mut machine = HookStateMachine::default();
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-1"),
    );
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "sessionEnd",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Forward(HookTransition::Release)
    );

    assert_eq!(
        machine.apply_event(AiTool::Cursor, "sessionStart", Some("session-1"), None),
        HookEventDecision::Ignore
    );
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "beforeSubmitPrompt",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Ignore
    );
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "beforeSubmitPrompt",
            Some("session-1"),
            Some("generation-2"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
}

#[test]
fn delayed_cursor_end_for_another_generation_cannot_end_current_work() {
    let mut machine = HookStateMachine::default();
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-2"),
    );

    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "sessionEnd",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Ignore
    );
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "stop",
            Some("session-1"),
            Some("generation-2"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
}

#[test]
fn stopped_turn_ignores_progress_without_a_generation_id() {
    let mut machine = HookStateMachine::default();
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-1"),
    );
    machine.apply_event(
        AiTool::Cursor,
        "stop",
        Some("session-1"),
        Some("generation-1"),
    );

    assert_eq!(
        machine.apply_event(AiTool::Cursor, "preToolUse", Some("session-1"), None),
        HookEventDecision::Ignore
    );
    assert!(!machine.sessions["session-1"].turn_active);
    assert_eq!(machine.sessions["session-1"].phase, HookPhase::Idle);
}

#[test]
fn cursor_subagent_events_never_claim_the_parent_generation() {
    let mut machine = HookStateMachine::default();
    machine.apply_event(AiTool::Cursor, "sessionStart", Some("session-1"), None);

    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "subagentStart",
            Some("session-1"),
            Some("conversation-id"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    assert_eq!(machine.sessions["session-1"].turn_id, None);
    assert_eq!(
        machine.apply_event_with_status(
            AiTool::Cursor,
            "subagentStop",
            Some("session-1"),
            Some("conversation-id"),
            Some("error"),
        ),
        HookEventDecision::Ignore
    );
    machine.apply_event(
        AiTool::Cursor,
        "preToolUse",
        Some("session-1"),
        Some("generation-1"),
    );
    assert_eq!(
        machine.sessions["session-1"].turn_id.as_deref(),
        Some("generation-1")
    );
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "stop",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
}

#[test]
fn idless_cursor_end_cannot_overtake_a_just_started_generation() {
    let mut machine = HookStateMachine::default();
    machine.apply_event_with_status_at(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-1"),
        None,
        Duration::from_secs(1),
    );

    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Cursor,
            "sessionEnd",
            Some("session-1"),
            None,
            None,
            Duration::from_millis(1_100),
        ),
        HookEventDecision::Ignore
    );
    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Cursor,
            "stop",
            Some("session-1"),
            Some("generation-1"),
            None,
            Duration::from_millis(1_200),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );

    let mut settled = HookStateMachine::default();
    settled.apply_event_with_status_at(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-2"),
        Some("generation-2"),
        None,
        Duration::from_secs(1),
    );
    assert_eq!(
        settled.apply_event_with_status_at(
            AiTool::Cursor,
            "sessionEnd",
            Some("session-2"),
            None,
            None,
            Duration::from_secs(2),
        ),
        HookEventDecision::Forward(HookTransition::Release)
    );
}

#[test]
fn idless_duplicate_terminal_cannot_relabel_a_finished_generation() {
    let mut machine = HookStateMachine::default();
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-1"),
    );
    machine.apply_event(
        AiTool::Cursor,
        "stop",
        Some("session-1"),
        Some("generation-1"),
    );
    assert_eq!(
        machine.apply_event_with_status(
            AiTool::Cursor,
            "stop",
            Some("session-1"),
            None,
            Some("error"),
        ),
        HookEventDecision::Ignore
    );
    assert_eq!(machine.sessions["session-1"].phase, HookPhase::Idle);

    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-2"),
    );
    machine.apply_event_with_status(
        AiTool::Cursor,
        "stop",
        Some("session-1"),
        Some("generation-2"),
        Some("error"),
    );
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "stop", Some("session-1"), None),
        HookEventDecision::Ignore
    );
    assert_eq!(machine.sessions["session-1"].phase, HookPhase::Error);
}
