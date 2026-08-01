use std::time::Duration;

use super::{
    HookEventDecision, HookPhase, HookSessionState, HookStateMachine, HookTransition,
    phase_decision, session::turn_is_stale,
};
pub(super) const DEFAULT_SESSION_KEY: &str = "__default__";

impl HookStateMachine {
    // workspaceOpen 是无会话 id 的占位信号。真实会话或结束墓碑一旦存在，迟到的
    // workspaceOpen 只能续期已有默认占位，不能创建第二个永久 Idle 会话。
    pub(super) fn apply_workspace_start(
        &mut self,
        observed_at: Duration,
        previous: HookPhase,
    ) -> HookEventDecision {
        if self.sessions.keys().any(|key| key != DEFAULT_SESSION_KEY) {
            return HookEventDecision::Ignore;
        }
        if let Some(existing) = self.sessions.get_mut(DEFAULT_SESSION_KEY) {
            if existing.ended {
                return HookEventDecision::Ignore;
            }
            existing.last_seen_at = observed_at;
            return phase_decision(previous, self.aggregate_phase());
        }
        self.sessions.insert(
            DEFAULT_SESSION_KEY.to_owned(),
            HookSessionState {
                phase: HookPhase::Idle,
                last_seen_at: observed_at,
                ..HookSessionState::default()
            },
        );
        phase_decision(previous, self.aggregate_phase())
    }

    // SessionEnd 建立终止墓碑。真实会话的首个生命周期事件也会清掉工作区占位，
    // 防止未知/乱序 End 因默认 Idle 会话而无法释放设备槽位。
    pub(super) fn apply_session_end(
        &mut self,
        session_key: String,
        turn_id: Option<&str>,
        reorder_grace: Duration,
        observed_at: Duration,
        previous: HookPhase,
    ) -> HookEventDecision {
        if self
            .sessions
            .get(&session_key)
            .is_some_and(|session| session.ended)
        {
            return HookEventDecision::Ignore;
        }
        // Cursor 的无 ID SessionEnd 可能与新 WorkStart 反向到达。仅保护刚刚明确
        // 开启且仍活跃的轮次；正常运行超过 grace 后的真实 End 仍立即终止。
        if turn_id.is_none()
            && !reorder_grace.is_zero()
            && self.sessions.get(&session_key).is_some_and(|session| {
                session.turn_active
                    && session.explicit_turn_started_at.is_some_and(|started_at| {
                        observed_at
                            .checked_sub(started_at)
                            .is_some_and(|elapsed| !elapsed.is_zero() && elapsed <= reorder_grace)
                    })
            })
        {
            return HookEventDecision::Ignore;
        }
        // 携带不同轮次 id 的 End 只证明该 incoming 轮次已结束，不能结束当前
        // 活跃轮次。无轮次 id 的 SessionEnd 仍按会话级终止信号处理。
        if let Some(session) = self.sessions.get_mut(&session_key)
            && session.turn_active
            && turn_is_stale(session, turn_id)
        {
            session.retire_observed_turn(turn_id);
            return HookEventDecision::Ignore;
        }
        if session_key != DEFAULT_SESSION_KEY {
            self.sessions.remove(DEFAULT_SESSION_KEY);
        }
        self.ensure_capacity_for(&session_key);
        let mut tombstone = self.sessions.remove(&session_key).unwrap_or_default();
        tombstone.finish_turn(turn_id);
        tombstone.phase = HookPhase::Released;
        tombstone.ended = true;
        tombstone.last_seen_at = observed_at;
        self.sessions.insert(session_key, tombstone);
        let next = self.aggregate_phase();
        if next == HookPhase::Released {
            return HookEventDecision::Forward(HookTransition::Release);
        }
        phase_decision(previous, next)
    }

    // SessionStart 只续期未结束会话。支持 resume 的协议可显式覆盖墓碑；Cursor
    // 会产生迟到重复 start，因此只允许新的明确 WorkStart 建立下一 epoch。
    pub(super) fn apply_session_start(
        &mut self,
        session_key: String,
        has_session_id: bool,
        revives_tombstone: bool,
        observed_at: Duration,
        previous: HookPhase,
    ) -> HookEventDecision {
        let is_ended = self
            .sessions
            .get(&session_key)
            .is_some_and(|session| session.ended);
        if is_ended && !revives_tombstone {
            return HookEventDecision::Ignore;
        }
        if has_session_id {
            self.sessions.remove(DEFAULT_SESSION_KEY);
        }
        if let Some(existing) = self.sessions.get_mut(&session_key) {
            if existing.ended {
                // resume 开启新的会话 epoch，但旧轮次历史仍需保留以拒绝上一个
                // epoch 的迟到事件；新的明确 WorkStart 会替换 current id。
                existing.ended = false;
                existing.phase = HookPhase::Idle;
                existing.turn_active = false;
            }
            existing.last_seen_at = observed_at;
            return phase_decision(previous, self.aggregate_phase());
        }
        self.ensure_capacity_for(&session_key);
        self.sessions.insert(
            session_key,
            HookSessionState {
                phase: HookPhase::Idle,
                last_seen_at: observed_at,
                ..HookSessionState::default()
            },
        );
        phase_decision(previous, self.aggregate_phase())
    }

    pub(super) fn remove_workspace_placeholder(&mut self) {
        self.sessions.remove(DEFAULT_SESSION_KEY);
    }
}
