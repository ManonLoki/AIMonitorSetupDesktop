use super::{HookEventDecision, HookStateMachine, HookTransition};
use crate::domain::monitor::{AiTool, HookBehavior};

#[test]
fn late_workspace_open_cannot_leave_a_real_session_placeholder() {
    let mut machine = HookStateMachine::default();

    assert_eq!(
        machine.apply_event(AiTool::Cursor, "sessionStart", Some("session-1"), None),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "workspaceOpen", None, None),
        HookEventDecision::Ignore
    );
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "sessionEnd", Some("session-1"), None),
        HookEventDecision::Forward(HookTransition::Release)
    );
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "workspaceOpen", None, None),
        HookEventDecision::Ignore
    );
}

#[test]
fn a_real_work_event_also_replaces_the_workspace_placeholder() {
    let mut machine = HookStateMachine::default();
    machine.apply_event(AiTool::Cursor, "workspaceOpen", None, None);

    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "beforeSubmitPrompt",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "sessionEnd", Some("session-1"), None),
        HookEventDecision::Forward(HookTransition::Release)
    );
}

#[test]
fn cursor_tool_failure_is_recoverable_within_the_same_generation() {
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
            "postToolUseFailure",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
    );
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "preToolUse",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
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
fn cursor_subagent_error_does_not_end_or_relabel_the_parent_generation() {
    let mut machine = HookStateMachine::default();
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-1"),
    );

    assert_eq!(
        machine.apply_event_with_status(
            AiTool::Cursor,
            "subagentStop",
            Some("session-1"),
            Some("subagent-generation"),
            Some("error"),
        ),
        HookEventDecision::Ignore
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
fn cursor_progress_without_generation_keeps_the_known_current_generation() {
    let mut machine = HookStateMachine::default();
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-1"),
    );

    assert_eq!(
        machine.apply_event(AiTool::Cursor, "preToolUse", Some("session-1"), None),
        HookEventDecision::Ignore
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
fn retired_cursor_generations_cannot_replace_or_stop_the_latest_generation() {
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
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-2"),
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
        machine.apply_event_with_status(
            AiTool::Cursor,
            "stop",
            Some("session-1"),
            Some("generation-1"),
            Some("error"),
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
fn explicit_generation_replacement_retires_the_previous_generation() {
    let mut machine = HookStateMachine::default();
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-1"),
    );
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-2"),
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
}

#[test]
fn stale_terminal_event_retires_its_generation_without_stopping_current_work() {
    let mut machine = HookStateMachine::default();
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-2"),
    );

    assert_eq!(
        machine.apply_event_with_status(
            AiTool::Cursor,
            "stop",
            Some("session-1"),
            Some("generation-1"),
            Some("error"),
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
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "beforeSubmitPrompt",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Ignore
    );
}

#[test]
fn mismatched_progress_does_not_block_a_later_explicit_work_start() {
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
            "preToolUse",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Ignore
    );
    machine.apply_event(
        AiTool::Cursor,
        "stop",
        Some("session-1"),
        Some("generation-2"),
    );
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "beforeSubmitPrompt",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
}

#[test]
fn first_observed_terminal_event_retires_its_generation() {
    let mut machine = HookStateMachine::default();

    assert_eq!(
        machine.apply_event_with_status(
            AiTool::Cursor,
            "stop",
            Some("session-1"),
            Some("generation-1"),
            Some("error"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
    );
    for event in ["preToolUse", "beforeSubmitPrompt"] {
        assert_eq!(
            machine.apply_event(
                AiTool::Cursor,
                event,
                Some("session-1"),
                Some("generation-1"),
            ),
            HookEventDecision::Ignore,
            "terminal generation must reject late {event}"
        );
    }
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "preToolUse",
            Some("session-1"),
            Some("generation-2"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
}

#[test]
fn retired_generation_history_is_bounded_per_session() {
    use super::session::MAX_RETIRED_TURNS_PER_SESSION;

    let mut machine = HookStateMachine::default();
    for index in 0..=MAX_RETIRED_TURNS_PER_SESSION {
        let turn = format!("generation-{index}");
        machine.apply_event(
            AiTool::Cursor,
            "beforeSubmitPrompt",
            Some("session-1"),
            Some(&turn),
        );
        machine.apply_event(AiTool::Cursor, "stop", Some("session-1"), Some(&turn));
    }

    let retired = &machine.sessions["session-1"].retired_turn_ids;
    assert_eq!(retired.len(), MAX_RETIRED_TURNS_PER_SESSION);
    assert!(!retired.iter().any(|turn| turn == "generation-0"));
    assert!(retired.iter().any(|turn| turn == "generation-1"));
    assert!(retired.iter().any(|turn| turn == "generation-256"));
}

#[test]
fn expired_tombstone_allows_a_fresh_session_with_the_same_id() {
    use std::time::Duration;
    let mut machine = HookStateMachine::default();
    machine.apply_event_with_status_at(
        AiTool::Cursor,
        "sessionEnd",
        Some("session-1"),
        None,
        None,
        Duration::from_secs(1),
    );
    assert_eq!(
        machine.expire_inactive_sessions(Duration::from_secs(12), Duration::from_secs(10)),
        HookEventDecision::Ignore
    );

    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Cursor,
            "sessionStart",
            Some("session-1"),
            None,
            None,
            Duration::from_secs(13),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
}
