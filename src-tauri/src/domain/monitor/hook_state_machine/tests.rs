use std::time::Duration;

use super::{HookEventDecision, HookPhase, HookStateMachine, HookTransition};
use crate::domain::monitor::{AiTool, HookBehavior};

#[test]
fn status_driven_protocols_map_native_states() {
    let mut hermes = HookStateMachine::default();
    assert_eq!(
        hermes.apply_event(
            AiTool::Hermes,
            "pre_llm_call",
            Some("session-1"),
            Some("turn-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    assert_eq!(
        hermes.apply_event(
            AiTool::Hermes,
            "pre_approval_request",
            Some("session-1"),
            Some("turn-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking))
    );

    let mut open_claw = HookStateMachine::default();
    open_claw.apply_event(AiTool::OpenClaw, "session_start", Some("s1"), None);
    open_claw.apply_event(AiTool::OpenClaw, "before_agent_run", Some("s1"), Some("r1"));
    assert_eq!(
        open_claw.apply_event_with_status(
            AiTool::OpenClaw,
            "agent_end",
            Some("s1"),
            Some("r1"),
            Some("failed"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
    );
}

#[test]
fn tools_outside_suppression_allowlist_forward_repeated_supported_events() {
    for (tool, event) in [
        (AiTool::WorkBuddy, "PreToolUse"),
        (AiTool::Hermes, "pre_llm_call"),
        (AiTool::OpenClaw, "before_agent_run"),
        (AiTool::CodeBuddy, "PreToolUse"),
    ] {
        let mut machine = HookStateMachine::default();
        for _ in 0..2 {
            assert_eq!(
                machine.apply_event(
                    tool,
                    event,
                    Some("passthrough-session"),
                    Some("arbitrary-turn"),
                ),
                HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running)),
                "{tool:?} event {event} should be forwarded"
            );
        }
    }
}

#[test]
fn codex_state_machine_covers_open_interrupt_late_completion_and_exit() {
    let mut machine = HookStateMachine::default();

    assert_eq!(
        machine.apply(AiTool::Codex, "SessionStart"),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(
        machine.apply(AiTool::Codex, "UserPromptSubmit"),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    assert_eq!(
        machine.apply(AiTool::Codex, "Stop"),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // Stop 之后不再依赖两秒窗口：无论迟到多久，完成类事件都不能重新进入运行态。
    assert_eq!(
        machine.apply(AiTool::Codex, "SubagentStop"),
        HookEventDecision::Ignore
    );
    assert_eq!(
        machine.apply(AiTool::Codex, "PostToolUse"),
        HookEventDecision::Ignore
    );
    assert_eq!(
        machine.apply(AiTool::Codex, "SessionEnd"),
        HookEventDecision::Forward(HookTransition::Release)
    );
}

#[test]
fn state_machine_only_resumes_after_a_real_work_start() {
    let mut machine = HookStateMachine::default();

    assert_eq!(
        machine.apply(AiTool::ClaudeCode, "Stop"),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(
        machine.apply(AiTool::ClaudeCode, "PostCompact"),
        HookEventDecision::Ignore
    );
    assert_eq!(
        machine.apply(AiTool::ClaudeCode, "UserPromptSubmit"),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // 活跃轮次中的完成事件保持 Running；由于展示状态未变化，无需重复打设备请求。
    assert_eq!(
        machine.apply(AiTool::ClaudeCode, "PostToolUse"),
        HookEventDecision::Ignore
    );
    assert_eq!(
        machine.apply(AiTool::ClaudeCode, "PermissionRequest"),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking))
    );
}

#[test]
fn duplicate_session_start_does_not_regress_an_active_session() {
    let mut machine = HookStateMachine::default();
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "SessionStart",
        Some("s1"),
        None,
        None,
        Duration::from_secs(1),
    );
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("s1"),
        Some("t1"),
        None,
        Duration::from_secs(2),
    );

    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "SessionStart",
            Some("s1"),
            None,
            None,
            Duration::from_secs(3),
        ),
        HookEventDecision::Ignore
    );
    assert_eq!(machine.sessions["s1"].phase, HookPhase::Running);
    assert!(machine.sessions["s1"].turn_active);
    assert_eq!(machine.sessions["s1"].last_seen_at, Duration::from_secs(3));
}

#[test]
fn cursor_stop_status_distinguishes_failure_from_completion() {
    let mut machine = HookStateMachine::default();

    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "beforeSubmitPrompt",
            Some("conversation-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    assert_eq!(
        machine.apply_event_with_status(
            AiTool::Cursor,
            "stop",
            Some("conversation-1"),
            Some("generation-1"),
            Some("error"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
    );

    let mut completed = HookStateMachine::default();
    completed.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("conversation-2"),
        Some("generation-2"),
    );
    assert_eq!(
        completed.apply_event_with_status(
            AiTool::Cursor,
            "stop",
            Some("conversation-2"),
            Some("generation-2"),
            Some("completed"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
}

#[test]
fn cursor_real_session_replaces_workspace_placeholder() {
    let mut machine = HookStateMachine::default();

    assert_eq!(
        machine.apply_event(AiTool::Cursor, "workspaceOpen", None, None),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "sessionStart", Some("conversation-1"), None,),
        HookEventDecision::Ignore
    );
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "sessionEnd", Some("conversation-1"), None,),
        HookEventDecision::Forward(HookTransition::Release)
    );
}

#[test]
fn state_machine_aggregates_multiple_sessions_without_cross_talk() {
    let mut machine = HookStateMachine::default();

    assert_eq!(
        machine.apply_event(AiTool::Codex, "SessionStart", Some("s1"), None),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(
        machine.apply_event(AiTool::Codex, "UserPromptSubmit", Some("s1"), Some("t1")),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // 第二个空闲会话出现时，第一个会话仍在工作，聚合状态保持 Running。
    assert_eq!(
        machine.apply_event(AiTool::Codex, "SessionStart", Some("s2"), None),
        HookEventDecision::Ignore
    );
    assert_eq!(
        machine.apply_event(AiTool::Codex, "Stop", Some("s1"), Some("t1")),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(
        machine.apply_event(AiTool::Codex, "UserPromptSubmit", Some("s2"), Some("t2")),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // 关闭 s1 不得释放仍在运行的 s2。
    assert_eq!(
        machine.apply_event(AiTool::Codex, "SessionEnd", Some("s1"), None),
        HookEventDecision::Ignore
    );
    assert_eq!(
        machine.apply_event(AiTool::Codex, "SessionEnd", Some("s2"), None),
        HookEventDecision::Forward(HookTransition::Release)
    );
}

#[test]
fn state_machine_rejects_events_from_an_older_turn() {
    let mut machine = HookStateMachine::default();
    machine.apply_event(AiTool::Codex, "SessionStart", Some("s1"), None);
    machine.apply_event(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("s1"),
        Some("new-turn"),
    );

    assert_eq!(
        machine.apply_event(AiTool::Codex, "Stop", Some("s1"), Some("old-turn")),
        HookEventDecision::Ignore
    );
    assert_eq!(
        machine.apply_event(AiTool::Codex, "Stop", Some("s1"), Some("new-turn")),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
}

#[test]
fn codex_goal_mode_resumes_with_a_new_turn_without_another_user_prompt() {
    let mut machine = HookStateMachine::default();
    machine.apply_event(AiTool::Codex, "SessionStart", Some("goal-session"), None);
    machine.apply_event(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("goal-session"),
        Some("turn-1"),
    );
    assert_eq!(
        machine.apply_event(AiTool::Codex, "Stop", Some("goal-session"), Some("turn-1"),),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );

    // 已停止轮次的迟到进度和完成事件都不能重新激活展示。
    for event in ["PreToolUse", "PostToolUse"] {
        assert_eq!(
            machine.apply_event(AiTool::Codex, event, Some("goal-session"), Some("turn-1"),),
            HookEventDecision::Ignore,
            "同一已停止轮次的迟到事件 {event} 应被抑制"
        );
    }

    // Goal 模式恢复不会再次提交用户 prompt；新 turn 的首个工作进度必须能
    // 建立隐式轮次并恢复 Running。
    assert_eq!(
        machine.apply_event(
            AiTool::Codex,
            "PreToolUse",
            Some("goal-session"),
            Some("turn-2"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    assert_eq!(
        machine.apply_event(AiTool::Codex, "Stop", Some("goal-session"), Some("turn-1"),),
        HookEventDecision::Ignore
    );
    assert_eq!(
        machine.apply_event(AiTool::Codex, "Stop", Some("goal-session"), Some("turn-2"),),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(
        machine.apply_event(
            AiTool::Codex,
            "PermissionRequest",
            Some("goal-session"),
            Some("turn-3"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking))
    );
    assert_eq!(
        machine.apply_event(AiTool::Codex, "Stop", Some("goal-session"), Some("turn-3"),),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
}
