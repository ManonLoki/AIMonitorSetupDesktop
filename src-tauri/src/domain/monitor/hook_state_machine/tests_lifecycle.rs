use std::time::Duration;

use super::{HookEventDecision, HookStateMachine, HookTransition, MAX_TRACKED_HOOK_SESSIONS};
use crate::domain::monitor::{AiTool, HookBehavior};

#[test]
fn orphan_completion_is_ignored_without_leaving_a_ghost_session() {
    let mut machine = HookStateMachine::default();

    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "PostToolUse",
            Some("late-session"),
            Some("turn-1"),
            None,
            Duration::from_secs(10),
        ),
        HookEventDecision::Ignore
    );
    assert_eq!(machine.tracked_session_count(), 0);

    // 与完成事件不同，真实工作进展可以作为 Monitor 中途启动后的首个事件。
    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "PreToolUse",
            Some("live-session"),
            Some("turn-1"),
            None,
            Duration::from_secs(11),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    assert_eq!(machine.tracked_session_count(), 1);
}

#[test]
fn ended_tombstone_rejects_late_events_until_explicit_restart() {
    let mut machine = HookStateMachine::default();
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("s1"),
        Some("t1"),
        None,
        Duration::from_secs(1),
    );
    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "SessionEnd",
            Some("s1"),
            None,
            None,
            Duration::from_secs(2),
        ),
        HookEventDecision::Forward(HookTransition::Release)
    );

    for event in [
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "PermissionRequest",
        "Stop",
    ] {
        assert_eq!(
            machine.apply_event_with_status_at(
                AiTool::Codex,
                event,
                Some("s1"),
                Some("t1"),
                None,
                Duration::from_secs(100),
            ),
            HookEventDecision::Ignore,
            "墓碑应拒绝迟到事件 {event}"
        );
    }
    assert_eq!(machine.sessions["s1"].last_seen_at, Duration::from_secs(2));

    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "SessionStart",
            Some("s1"),
            None,
            None,
            Duration::from_secs(101),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    assert!(!machine.sessions["s1"].ended);
}

#[test]
fn unknown_session_end_releases_once_without_overriding_other_live_sessions() {
    let mut machine = HookStateMachine::default();

    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "SessionEnd",
            Some("unknown"),
            None,
            None,
            Duration::from_secs(1),
        ),
        HookEventDecision::Forward(HookTransition::Release)
    );
    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "SessionEnd",
            Some("unknown"),
            None,
            None,
            Duration::from_secs(2),
        ),
        HookEventDecision::Ignore
    );
    assert_eq!(
        machine.sessions["unknown"].last_seen_at,
        Duration::from_secs(1)
    );

    machine.apply_event_with_status_at(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("live"),
        Some("turn"),
        None,
        Duration::from_secs(3),
    );
    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "SessionEnd",
            Some("another-unknown"),
            None,
            None,
            Duration::from_secs(4),
        ),
        HookEventDecision::Ignore
    );
}

#[test]
fn expiring_sessions_batches_changes_into_one_final_aggregate_transition() {
    let mut machine = HookStateMachine::default();
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("asking"),
        Some("t1"),
        None,
        Duration::ZERO,
    );
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "PermissionRequest",
        Some("asking"),
        Some("t1"),
        None,
        Duration::ZERO,
    );
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("running"),
        Some("t2"),
        None,
        Duration::from_secs(5),
    );
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "SessionStart",
        Some("idle"),
        None,
        None,
        Duration::from_secs(15),
    );

    // Asking 和 Running 同批到期；对外只暴露最终仍存活的 Idle 聚合态。
    assert_eq!(
        machine.expire_inactive_sessions(Duration::from_secs(20), Duration::from_secs(10),),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(machine.tracked_session_count(), 1);
    assert_eq!(
        machine.expire_inactive_sessions(Duration::from_secs(26), Duration::from_secs(10),),
        HookEventDecision::Forward(HookTransition::Release)
    );
    assert_eq!(machine.tracked_session_count(), 0);
}

#[test]
fn session_tracking_stays_bounded_and_uses_eviction_priority() {
    let mut machine = HookStateMachine::default();
    for index in 0..MAX_TRACKED_HOOK_SESSIONS {
        let session_id = format!("session-{index:03}");
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "UserPromptSubmit",
            Some(&session_id),
            Some("turn"),
            None,
            Duration::from_secs(index as u64),
        );
    }
    assert_eq!(machine.tracked_session_count(), MAX_TRACKED_HOOK_SESSIONS);

    // 即使墓碑较新，也应先于任何活跃会话淘汰。
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "SessionEnd",
        Some("session-000"),
        None,
        None,
        Duration::from_mins(5),
    );
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("overflow-1"),
        Some("turn"),
        None,
        Duration::from_secs(301),
    );
    assert!(!machine.sessions.contains_key("session-000"));

    // 非活跃会话其次；只有两类都不存在时才淘汰最旧活跃会话。
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "Stop",
        Some("session-001"),
        Some("turn"),
        None,
        Duration::from_secs(302),
    );
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("overflow-2"),
        Some("turn"),
        None,
        Duration::from_secs(303),
    );
    assert!(!machine.sessions.contains_key("session-001"));
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("overflow-3"),
        Some("turn"),
        None,
        Duration::from_secs(304),
    );
    assert!(!machine.sessions.contains_key("session-002"));
    assert_eq!(machine.tracked_session_count(), MAX_TRACKED_HOOK_SESSIONS);
}
