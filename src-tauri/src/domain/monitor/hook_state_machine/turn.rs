use std::time::Duration;

use super::{
    HookEventDecision, HookPhase, HookSessionState, HookStateMachine, HookTransition,
    phase_decision,
    session::{StoppedTurnDecision, stopped_turn_decision, turn_is_stale},
};
use crate::domain::monitor::{HookBehavior, hooks::HookEventKind};

impl HookStateMachine {
    pub(super) fn apply_turn_event(
        &mut self,
        session_key: String,
        has_session_id: bool,
        event_kind: HookEventKind,
        turn_id: Option<&str>,
        observed_at: Duration,
        previous: HookPhase,
    ) -> HookEventDecision {
        // 结束墓碑只允许带未退休 id 的明确 WorkStart 建立新 epoch；这既拒绝
        // 迟到旧 generation，也支持 Cursor 重新进入同 conversation 后直接提交。
        if !self.accept_ended_session_event(&session_key, event_kind, turn_id) {
            return HookEventDecision::Ignore;
        }

        // 完成类事件不是可靠的工作起点。Monitor 若中途启动、没有见过对应
        // 会话或工作开始，则直接忽略且不留下幽灵记录。
        if matches!(
            event_kind,
            HookEventKind::WorkCompletion(_) | HookEventKind::UnscopedWorkCompletion
        ) && !self.sessions.contains_key(&session_key)
        {
            return HookEventDecision::Ignore;
        }

        // workspaceOpen 的默认占位只负责冷启动展示。任何被接纳的真实会话事件
        // 都会接管该位置，避免真实 SessionEnd 后残留永久 Idle 占位。
        if has_session_id {
            self.remove_workspace_placeholder();
        }
        self.ensure_capacity_for(&session_key);
        let session = self
            .sessions
            .entry(session_key)
            .or_insert_with(|| HookSessionState {
                last_seen_at: observed_at,
                ..HookSessionState::default()
            });

        if matches!(event_kind, HookEventKind::UnscopedWorkCompletion) {
            if session.turn_active {
                session.last_seen_at = observed_at;
            }
            return HookEventDecision::Ignore;
        }

        // Cursor 子代理 generation 与父 generation 无关。它只能在尚无任何已知
        // 父轮次时作为冷启动工作信号；已有父轮次时不改写 id、状态或优先级。
        if matches!(event_kind, HookEventKind::UnscopedWorkStart(_)) {
            if session.turn_active {
                session.last_seen_at = observed_at;
                return HookEventDecision::Ignore;
            }
            if session.turn_id.is_some() {
                return HookEventDecision::Ignore;
            }
            session.turn_active = true;
            session.phase = HookPhase::Running;
            session.last_seen_at = observed_at;
            return phase_decision(previous, self.aggregate_phase());
        }

        if event_kind == HookEventKind::WorkStart {
            if session.is_retired_turn(turn_id) {
                return HookEventDecision::Ignore;
            }
            session.start_explicit_turn(turn_id, observed_at);
            session.phase = HookPhase::Running;
            session.last_seen_at = observed_at;
            return phase_decision(previous, self.aggregate_phase());
        }

        let transition = event_kind.transition();
        if event_kind == HookEventKind::Stop
            || matches!(event_kind, HookEventKind::TerminalState(_))
        {
            if !session.turn_active
                && turn_id.is_none()
                && session.is_retired_turn(session.turn_id.as_deref())
            {
                return HookEventDecision::Ignore;
            }
            if session.is_retired_turn(turn_id) {
                return HookEventDecision::Ignore;
            }
            if turn_is_stale(session, turn_id) {
                session.retire_observed_turn(turn_id);
                return HookEventDecision::Ignore;
            }
            session.finish_turn(turn_id);
            session.phase = match transition {
                HookTransition::Display(HookBehavior::Error) => HookPhase::Error,
                _ => HookPhase::Idle,
            };
            session.last_seen_at = observed_at;
            return phase_decision(previous, self.aggregate_phase());
        }

        if session.is_retired_turn(turn_id) || session.is_quarantined_turn(turn_id) {
            return HookEventDecision::Ignore;
        }
        if matches!(event_kind, HookEventKind::WorkCompletion(_)) && !session.turn_active {
            return HookEventDecision::Ignore;
        }

        // Goal 模式暂停/恢复或自动续跑时可能没有新的 WorkStart。已停止会话收到
        // 不同且未终止 turn 的明确进度/询问/异常时，将其作为隐式工作起点。
        let starts_new_implicit_turn = match stopped_turn_decision(session, event_kind, turn_id) {
            StoppedTurnDecision::SuppressLateEvent => return HookEventDecision::Ignore,
            StoppedTurnDecision::StartNewTurn => true,
            StoppedTurnDecision::NotApplicable => false,
        };
        if turn_is_stale(session, turn_id) && !starts_new_implicit_turn {
            session.quarantine_observed_turn(turn_id);
            return HookEventDecision::Ignore;
        }
        if starts_new_implicit_turn {
            session.start_turn(turn_id);
        }

        let next = next_turn_phase(session, event_kind, transition, turn_id);
        session.phase = next;
        session.last_seen_at = observed_at;
        phase_decision(previous, self.aggregate_phase())
    }

    fn accept_ended_session_event(
        &mut self,
        session_key: &str,
        event_kind: HookEventKind,
        turn_id: Option<&str>,
    ) -> bool {
        let Some(session) = self
            .sessions
            .get_mut(session_key)
            .filter(|session| session.ended)
        else {
            return true;
        };
        if event_kind != HookEventKind::WorkStart
            || turn_id.is_none()
            || session.is_retired_turn(turn_id)
        {
            return false;
        }
        session.ended = false;
        true
    }
}

fn next_turn_phase(
    session: &mut HookSessionState,
    event_kind: HookEventKind,
    transition: HookTransition,
    turn_id: Option<&str>,
) -> HookPhase {
    match transition {
        HookTransition::Release => HookPhase::Released,
        HookTransition::Display(HookBehavior::Idle) => HookPhase::Idle,
        HookTransition::Display(HookBehavior::Running) => {
            session.continue_turn(turn_id);
            HookPhase::Running
        }
        HookTransition::Display(HookBehavior::Asking) => {
            session.continue_turn(turn_id);
            HookPhase::Asking
        }
        HookTransition::Display(HookBehavior::Error) => {
            if matches!(event_kind, HookEventKind::WorkProgress(_)) {
                session.continue_turn(turn_id);
            } else {
                session.finish_turn(turn_id);
            }
            HookPhase::Error
        }
    }
}
