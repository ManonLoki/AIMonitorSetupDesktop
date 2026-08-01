use std::{collections::VecDeque, time::Duration};

use super::super::hooks::HookEventKind;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct HookSessionState {
    pub(super) phase: HookPhase,
    pub(super) turn_active: bool,
    pub(super) turn_id: Option<String>,
    /// 已终止、已判旧或被新明确工作起点取代的 turn。Cursor generation id 等上游标识
    /// 是不透明字符串，只能用已观察历史拒绝其迟到事件，因此必须同时设上限。
    pub(super) retired_turn_ids: VecDeque<String>,
    /// 当前轮次活跃期间抢先到达的不同 turn id。它们的先后关系无法从不透明 ID
    /// 推断，因此只隔离普通进度；后续明确 `WorkStart` 仍可接纳并解除隔离。
    pub(super) quarantined_turn_ids: VecDeque<String>,
    /// 已收到 `SessionEnd` 的会话保留为空墓碑，用于拒绝随后迟到的事件。
    pub(super) ended: bool,
    /// 由应用层注入的进程内单调经过时间，领域层不直接读取系统时钟。
    pub(super) last_seen_at: Duration,
    /// 最近一次明确 `WorkStart` 的接收时间，仅用于消歧 Cursor 无轮次 ID 的反向 End。
    pub(super) explicit_turn_started_at: Option<Duration>,
}

pub(super) const MAX_RETIRED_TURNS_PER_SESSION: usize = 256;

impl HookSessionState {
    pub(super) fn is_retired_turn(&self, turn_id: Option<&str>) -> bool {
        turn_id.is_some_and(|turn_id| {
            self.retired_turn_ids
                .iter()
                .any(|retired| retired == turn_id)
        })
    }

    pub(super) fn is_quarantined_turn(&self, turn_id: Option<&str>) -> bool {
        turn_id.is_some_and(|turn_id| {
            self.quarantined_turn_ids
                .iter()
                .any(|quarantined| quarantined == turn_id)
        })
    }

    /// 明确开启一个轮次；即使新事件没有 id，也要终止并清空旧的可识别轮次。
    pub(super) fn start_turn(&mut self, turn_id: Option<&str>) {
        if self.turn_id.as_deref() != turn_id {
            if let Some(previous) = self.turn_id.take() {
                self.retire_turn(previous);
            }
            self.turn_id = turn_id.map(str::to_owned);
        }
        self.turn_active = true;
    }

    /// 明确工作起点是可信的 epoch 边界，可以接纳此前因乱序而暂时隔离的 id。
    pub(super) fn start_explicit_turn(&mut self, turn_id: Option<&str>, observed_at: Duration) {
        self.quarantined_turn_ids.clear();
        self.start_turn(turn_id);
        self.explicit_turn_started_at = Some(observed_at);
    }

    /// 延续当前轮次。部分 Cursor 进度事件不携带 `generation_id`，此时必须保留
    /// 已知 current id；只有尚无 current id 时才从进度信号补全。
    pub(super) fn continue_turn(&mut self, turn_id: Option<&str>) {
        if self.turn_id.is_none() {
            self.turn_id = turn_id.map(str::to_owned);
        }
        self.turn_active = true;
    }

    pub(super) fn finish_turn(&mut self, turn_id: Option<&str>) {
        let finished = turn_id.map(str::to_owned).or_else(|| self.turn_id.clone());
        if let Some(finished) = finished {
            self.turn_id = Some(finished.clone());
            self.retire_turn(finished);
        }
        self.turn_active = false;
        // 不透明 ID 的隔离只对当时的活跃 current 有效。current 已明确结束后，
        // 同一候选再次到达可作为没有 WorkStart 的 Goal 续跑信号重新判断。
        self.quarantined_turn_ids.clear();
        self.explicit_turn_started_at = None;
    }

    pub(super) fn retire_observed_turn(&mut self, turn_id: Option<&str>) {
        if let Some(turn_id) = turn_id {
            self.retire_turn(turn_id.to_owned());
        }
    }

    pub(super) fn quarantine_observed_turn(&mut self, turn_id: Option<&str>) {
        let Some(turn_id) = turn_id else {
            return;
        };
        if self.is_retired_turn(Some(turn_id))
            || self
                .quarantined_turn_ids
                .iter()
                .any(|quarantined| quarantined == turn_id)
        {
            return;
        }
        if self.quarantined_turn_ids.len() == MAX_RETIRED_TURNS_PER_SESSION {
            self.quarantined_turn_ids.pop_front();
        }
        self.quarantined_turn_ids.push_back(turn_id.to_owned());
    }

    fn retire_turn(&mut self, turn_id: String) {
        self.quarantined_turn_ids
            .retain(|quarantined| quarantined != &turn_id);
        if self
            .retired_turn_ids
            .iter()
            .any(|retired| retired == &turn_id)
        {
            return;
        }
        if self.retired_turn_ids.len() == MAX_RETIRED_TURNS_PER_SESSION {
            self.retired_turn_ids.pop_front();
        }
        self.retired_turn_ids.push_back(turn_id);
    }
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

// 同一已停止/隔离轮次的进度是迟到事件；未观察过的不同轮次则是 Goal 模式
// 恢复或自动续跑。无 id 的进度不能越过一个已有可识别终止轮次。
pub(super) fn stopped_turn_decision(
    session: &HookSessionState,
    event_kind: HookEventKind,
    turn_id: Option<&str>,
) -> StoppedTurnDecision {
    if session.turn_active
        || !matches!(
            event_kind,
            HookEventKind::WorkProgress(_) | HookEventKind::State(_)
        )
    {
        return StoppedTurnDecision::NotApplicable;
    }
    if turn_id.is_none() {
        return if session.turn_id.is_some() {
            StoppedTurnDecision::SuppressLateEvent
        } else {
            StoppedTurnDecision::NotApplicable
        };
    }
    if session.is_retired_turn(turn_id) || session.is_quarantined_turn(turn_id) {
        StoppedTurnDecision::SuppressLateEvent
    } else if session.turn_id.is_none() || turn_is_stale(session, turn_id) {
        StoppedTurnDecision::StartNewTurn
    } else {
        StoppedTurnDecision::SuppressLateEvent
    }
}
