// 标准库：HashMap 用于按会话聚合状态，Duration 用于进程内单调时钟。
use std::{collections::HashMap, time::Duration};

use super::device::{AiTool, HookBehavior};
use super::hooks::{
    HookEventKind, event_kind, forwards_every_event, release_settle_delay,
    session_start_revives_tombstone,
};

mod lifecycle;
mod session;
mod turn;

use lifecycle::DEFAULT_SESSION_KEY;
use session::{HookPhase, HookSessionState, session_eviction_priority};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_cursor_reentry;
#[cfg(test)]
mod tests_cursor_timing;
#[cfg(test)]
mod tests_lifecycle;
#[cfg(test)]
mod tests_session_resume;

// 一个 Hook 事件触发后，展示屏应执行的动作：切换到某个行为展示，或者释放（退出）该展示位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookTransition {
    Display(HookBehavior),
    Release,
}

/// Hook 事件经过生命周期算法后的处理决定。应用层只负责执行 `Forward`，
/// `Ignore` 表示这是重复或已经失去时序意义的事件，`Unsupported` 表示配置/请求
/// 中出现了该工具不认识的事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEventDecision {
    Forward(HookTransition),
    Ignore,
    Unsupported,
}

/// 单个工具最多保留的 Hook 会话数。结束墓碑也计入上限，避免缺失
/// `SessionEnd` 或监控进程中途启动时，无界积累会话状态。
pub(crate) const MAX_TRACKED_HOOK_SESSIONS: usize = 256;

/// 单个 AI 工具的生命周期状态。它不依赖墙上时钟，因此迟到多久的完成事件都
/// 不会越过已经收到的 Stop/SessionEnd，把监控屏错误地切回运行中。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HookStateMachine {
    sessions: HashMap<String, HookSessionState>,
}

impl HookStateMachine {
    /// 把原生 Hook 事件归一化后推进状态机，并返回唯一需要由应用层执行的动作。
    #[cfg(test)]
    pub fn apply(&mut self, tool: AiTool, event: &str) -> HookEventDecision {
        self.apply_event(tool, event, None, None)
    }

    /// 带会话/轮次标识推进状态。多会话共享同一个工具展示位时，以所有会话的
    /// 聚合状态为准；旧 turn 的迟到事件只会影响它自己的会话，且会被忽略。
    #[cfg(test)]
    pub fn apply_event(
        &mut self,
        tool: AiTool,
        event: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
    ) -> HookEventDecision {
        self.apply_event_with_status(tool, event, session_id, turn_id, None)
    }

    /// Cursor 的 `stop` 通过 `status` 区分正常完成和异常结束；其他工具当前由
    /// 独立事件表达错误。协议差异由工具适配器解析，状态机只消费归一化类别。
    #[cfg(test)]
    pub fn apply_event_with_status(
        &mut self,
        tool: AiTool,
        event: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        status: Option<&str>,
    ) -> HookEventDecision {
        self.apply_event_with_status_at(tool, event, session_id, turn_id, status, Duration::ZERO)
    }

    /// 使用调用方提供的单调经过时间推进状态机。只有被接纳的事件会刷新
    /// `last_seen_at`；被墓碑或轮次时序拒绝的迟到事件不能延长记录寿命。
    pub(crate) fn apply_event_with_status_at(
        &mut self,
        tool: AiTool,
        event: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        status: Option<&str>,
        observed_at: Duration,
    ) -> HookEventDecision {
        let Some(event_kind) = event_kind(tool, event, status) else {
            return HookEventDecision::Unsupported;
        };
        if forwards_every_event(tool) {
            return HookEventDecision::Forward(event_kind.transition());
        }
        let previous = self.aggregate_phase();

        if event_kind == HookEventKind::WorkspaceStart {
            return self.apply_workspace_start(observed_at, previous);
        }

        let session_key = session_id.unwrap_or(DEFAULT_SESSION_KEY).to_owned();

        if event_kind == HookEventKind::SessionEnd {
            return self.apply_session_end(
                session_key,
                turn_id,
                release_settle_delay(tool),
                observed_at,
                previous,
            );
        }

        if event_kind == HookEventKind::SessionStart {
            return self.apply_session_start(
                session_key,
                session_id.is_some(),
                session_start_revives_tombstone(tool),
                observed_at,
                previous,
            );
        }

        self.apply_turn_event(
            session_key,
            session_id.is_some(),
            event_kind,
            turn_id,
            observed_at,
            previous,
        )
    }

    /// 一次性清理所有到期记录，并只根据清理前后的最终聚合状态返回一个决定。
    /// 超时仅用于回收内部记录，不能等同于明确的 `SessionEnd` 并释放设备槽位：
    /// 最后一个空闲会话到期时保留设备上的空闲内容，非空闲状态则回落为空闲展示。
    /// 使用 `saturating_sub` 防御调用方意外传入较小时间值，避免下溢。
    pub(crate) fn expire_inactive_sessions(
        &mut self,
        observed_at: Duration,
        timeout: Duration,
    ) -> HookEventDecision {
        let previous = self.aggregate_phase();
        self.sessions
            .retain(|_, session| observed_at.saturating_sub(session.last_seen_at) < timeout);
        let next = self.aggregate_phase();
        if next != HookPhase::Released {
            return phase_decision(previous, next);
        }

        match previous {
            HookPhase::Running | HookPhase::Asking | HookPhase::Error => {
                HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
            }
            HookPhase::Released | HookPhase::Idle => HookEventDecision::Ignore,
        }
    }

    /// 返回当前聚合展示状态，供新上线或地址发生变化的设备补齐它离线期间错过的
    /// 状态。Released 不需要重放：新设备没有本控制端创建的槽位，发送 DELETE
    /// 既无意义，也可能错误清理刚建立的继任展示。
    pub(crate) fn current_display_transition(&self) -> Option<HookTransition> {
        let phase = self.aggregate_phase();
        (phase != HookPhase::Released).then(|| phase_transition(phase))
    }

    /// 为新会话腾出一个位置：优先淘汰最旧墓碑，其次最旧非活跃会话，
    /// 最后才淘汰最旧活跃会话。相同时间以会话键排序，保证行为可复现。
    fn ensure_capacity_for(&mut self, session_key: &str) {
        if self.sessions.contains_key(session_key) {
            return;
        }
        while self.sessions.len() >= MAX_TRACKED_HOOK_SESSIONS {
            let Some(eviction_key) = self
                .sessions
                .iter()
                .min_by(|(left_key, left), (right_key, right)| {
                    session_eviction_priority(left)
                        .cmp(&session_eviction_priority(right))
                        .then_with(|| left.last_seen_at.cmp(&right.last_seen_at))
                        .then_with(|| left_key.cmp(right_key))
                })
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.sessions.remove(&eviction_key);
        }
    }

    // 把所有活跃会话的独立状态聚合成设备展示位的唯一目标状态：
    // 优先级 Asking > Error > Running > Idle，没有存活会话则视为已释放。
    fn aggregate_phase(&self) -> HookPhase {
        [
            HookPhase::Asking,
            HookPhase::Error,
            HookPhase::Running,
            HookPhase::Idle,
        ]
        .into_iter()
        .find(|phase| {
            self.sessions
                .values()
                .any(|session| !session.ended && session.phase == *phase)
        })
        .unwrap_or(HookPhase::Released)
    }

    #[cfg(test)]
    fn tracked_session_count(&self) -> usize {
        self.sessions.len()
    }
}

// 把聚合状态的变化转换成应用层需要执行的动作：状态未变则忽略，
// 变化了才转换为对应的展示/释放转换。
fn phase_decision(previous: HookPhase, next: HookPhase) -> HookEventDecision {
    if previous == next {
        return HookEventDecision::Ignore;
    }
    HookEventDecision::Forward(phase_transition(next))
}

fn phase_transition(phase: HookPhase) -> HookTransition {
    match phase {
        HookPhase::Released => HookTransition::Release,
        HookPhase::Idle => HookTransition::Display(HookBehavior::Idle),
        HookPhase::Running => HookTransition::Display(HookBehavior::Running),
        HookPhase::Asking => HookTransition::Display(HookBehavior::Asking),
        HookPhase::Error => HookTransition::Display(HookBehavior::Error),
    }
}
