use std::time::Duration;

use super::super::hooks::HookEventKind;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct HookSessionState {
    pub(super) phase: HookPhase,
    pub(super) turn_active: bool,
    pub(super) turn_id: Option<String>,
    /// 已收到 `SessionEnd` 的会话保留为空墓碑，用于拒绝随后迟到的事件。
    pub(super) ended: bool,
    /// 由应用层注入的进程内单调经过时间，领域层不直接读取系统时钟。
    pub(super) last_seen_at: Duration,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum HookPhase {
    #[default]
    Released,
    Idle,
    Running,
    Asking,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StoppedTurnDecision {
    NotApplicable,
    SuppressLateEvent,
    StartNewTurn,
}

// 会话被状态机淘汰的优先级：墓碑最先，其次是已停止的非活跃会话，
// 正在进行中的活跃会话最后。
pub(super) fn session_eviction_priority(session: &HookSessionState) -> u8 {
    if session.ended {
        0
    } else if !session.turn_active {
        1
    } else {
        2
    }
}

// 没有轮次 id 的事件永远不算过期；存在轮次 id 时只接受当前轮次。
pub(super) fn turn_is_stale(session: &HookSessionState, incoming_turn_id: Option<&str>) -> bool {
    incoming_turn_id.is_some_and(|incoming| {
        session
            .turn_id
            .as_deref()
            .is_some_and(|current| current != incoming)
    })
}

// 同一已停止轮次的进度是迟到事件；不同轮次则是 Goal 模式恢复或自动续跑。
pub(super) fn stopped_turn_decision(
    session: &HookSessionState,
    event_kind: HookEventKind,
    turn_id: Option<&str>,
) -> StoppedTurnDecision {
    if session.turn_active
        || turn_id.is_none()
        || session.turn_id.is_none()
        || !matches!(
            event_kind,
            HookEventKind::WorkProgress(_) | HookEventKind::State(_)
        )
    {
        return StoppedTurnDecision::NotApplicable;
    }
    if turn_is_stale(session, turn_id) {
        StoppedTurnDecision::StartNewTurn
    } else {
        StoppedTurnDecision::SuppressLateEvent
    }
}
