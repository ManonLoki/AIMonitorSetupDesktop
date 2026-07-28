// 标准库：HashMap 用于按会话聚合状态，Duration 用于进程内单调时钟。
use std::{collections::HashMap, time::Duration};

use super::device::{AiTool, HookBehavior};
use super::hooks::{HookEventKind, event_kind, forwards_every_event};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_lifecycle;

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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct HookSessionState {
    phase: HookPhase,
    turn_active: bool,
    turn_id: Option<String>,
    /// 已收到 `SessionEnd` 的会话保留为空墓碑，用于拒绝随后迟到的事件。
    ended: bool,
    /// 由应用层注入的进程内单调经过时间，领域层不直接读取系统时钟。
    last_seen_at: Duration,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum HookPhase {
    #[default]
    Released,
    Idle,
    Running,
    Asking,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StoppedTurnDecision {
    NotApplicable,
    SuppressLateEvent,
    StartNewTurn,
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
        let transition = event_kind.transition();
        if forwards_every_event(tool) {
            return HookEventDecision::Forward(transition);
        }
        let previous = self.aggregate_phase();
        let session_key = session_id.unwrap_or("__default__").to_owned();

        if event_kind == HookEventKind::SessionEnd {
            return self.apply_session_end(session_key, observed_at, previous);
        }

        if event_kind == HookEventKind::SessionStart {
            return self.apply_session_start(
                session_key,
                session_id.is_some(),
                observed_at,
                previous,
            );
        }

        // 结束墓碑只允许显式 SessionStart 覆盖；任何迟到事件（包括重复 Stop
        // 和新的工作事件）均不刷新墓碑时间，也不能隐式复活会话。
        if self
            .sessions
            .get(&session_key)
            .is_some_and(|session| session.ended)
        {
            return HookEventDecision::Ignore;
        }

        // 完成类事件不是可靠的工作起点。Monitor 若中途启动、没有见过对应
        // 会话或工作开始，则直接忽略且不留下幽灵记录。
        if matches!(event_kind, HookEventKind::WorkCompletion(_))
            && !self.sessions.contains_key(&session_key)
        {
            return HookEventDecision::Ignore;
        }

        self.ensure_capacity_for(&session_key);
        let session = self
            .sessions
            .entry(session_key)
            .or_insert_with(|| HookSessionState {
                last_seen_at: observed_at,
                ..HookSessionState::default()
            });

        if event_kind == HookEventKind::WorkStart {
            session.turn_active = true;
            session.turn_id = turn_id.map(str::to_owned);
            session.phase = HookPhase::Running;
            session.last_seen_at = observed_at;
            return phase_decision(previous, self.aggregate_phase());
        }

        if event_kind == HookEventKind::Stop {
            if turn_is_stale(session, turn_id) {
                return HookEventDecision::Ignore;
            }
            session.turn_active = false;
            session.phase = HookPhase::Idle;
            session.last_seen_at = observed_at;
            return phase_decision(previous, self.aggregate_phase());
        }

        if matches!(event_kind, HookEventKind::WorkCompletion(_))
            && (!session.turn_active || turn_is_stale(session, turn_id))
        {
            return HookEventDecision::Ignore;
        }

        // Goal 模式暂停/恢复或自动续跑时，Codex 会在同一会话内创建新 turn，
        // 但不会再次产生 UserPromptSubmit。已停止会话收到不同 turn 的明确进度、
        // 询问或异常时，应把它视为新的隐式工作起点；同一 turn 的迟到事件仍需
        // 抑制，避免 Stop 后被旧 PreToolUse 等进度事件重新激活。
        let starts_new_implicit_turn = match stopped_turn_decision(session, event_kind, turn_id) {
            StoppedTurnDecision::SuppressLateEvent => return HookEventDecision::Ignore,
            StoppedTurnDecision::StartNewTurn => true,
            StoppedTurnDecision::NotApplicable => false,
        };

        if turn_is_stale(session, turn_id) && !starts_new_implicit_turn {
            return HookEventDecision::Ignore;
        }
        if starts_new_implicit_turn {
            session.turn_id = turn_id.map(str::to_owned);
        }
        let next = match transition {
            HookTransition::Release => HookPhase::Released,
            HookTransition::Display(HookBehavior::Idle) => HookPhase::Idle,
            HookTransition::Display(HookBehavior::Running) => {
                session.turn_active = true;
                if let Some(turn_id) = turn_id {
                    session.turn_id = Some(turn_id.to_owned());
                }
                HookPhase::Running
            }
            HookTransition::Display(HookBehavior::Asking) => {
                session.turn_active = true;
                if let Some(turn_id) = turn_id {
                    session.turn_id = Some(turn_id.to_owned());
                }
                HookPhase::Asking
            }
            HookTransition::Display(HookBehavior::Error) => {
                session.turn_active = false;
                HookPhase::Error
            }
        };
        session.phase = next;
        session.last_seen_at = observed_at;
        phase_decision(previous, self.aggregate_phase())
    }

    // 把会话标记为已结束的空墓碑（保留记录但清空活跃状态），拒绝后续迟到事件
    // 隐式复活它；重复的 SessionEnd 直接忽略，不重复触发释放动作。
    fn apply_session_end(
        &mut self,
        session_key: String,
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

        self.ensure_capacity_for(&session_key);
        self.sessions.insert(
            session_key,
            HookSessionState {
                phase: HookPhase::Released,
                turn_active: false,
                turn_id: None,
                ended: true,
                last_seen_at: observed_at,
            },
        );
        let next = self.aggregate_phase();
        // 即使 Monitor 在会话开始后才启动，也要让首次结束事件向目标设备
        // 幂等释放一次。墓碑保证同一结束事件重放时不会形成请求风暴。
        if next == HookPhase::Released {
            return HookEventDecision::Forward(HookTransition::Release);
        }
        phase_decision(previous, next)
    }

    // 建立/续存一个会话记录：已存在且未结束的同一会话只续期存活时间，
    // 不会把正在展示的非空闲状态错误地拉回空闲。
    fn apply_session_start(
        &mut self,
        session_key: String,
        has_session_id: bool,
        observed_at: Duration,
        previous: HookPhase,
    ) -> HookEventDecision {
        // Cursor 的 workspaceOpen 不带 conversation_id，先以默认占位展示空闲；
        // 真正的 sessionStart 到来后必须替换该占位，否则 SessionEnd 后会残留
        // 一个永远无法释放的“工作区会话”。
        if has_session_id {
            self.sessions.remove("__default__");
        }
        // 同一会话的重复或迟到 SessionStart 只算作存活信号，不能把已经
        // Running/Asking/Error 的状态倒退回 Idle；真正结束后的 tombstone
        // 仍允许由显式 SessionStart 覆盖，支持会话 ID 被上游重新使用。
        if let Some(existing) = self.sessions.get_mut(&session_key)
            && !existing.ended
        {
            existing.last_seen_at = observed_at;
            return phase_decision(previous, self.aggregate_phase());
        }
        self.ensure_capacity_for(&session_key);
        self.sessions.insert(
            session_key,
            HookSessionState {
                phase: HookPhase::Idle,
                turn_active: false,
                turn_id: None,
                ended: false,
                last_seen_at: observed_at,
            },
        );
        phase_decision(previous, self.aggregate_phase())
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

// 会话被 `ensure_capacity_for` 淘汰的优先级：墓碑最先被淘汰，其次是
// 轮次已结束的非活跃会话，正在进行中的活跃会话最后才被淘汰。
fn session_eviction_priority(session: &HookSessionState) -> u8 {
    if session.ended {
        0
    } else if !session.turn_active {
        1
    } else {
        2
    }
}

// 判断收到的事件是否来自一个已经被更新轮次替换掉的旧轮次；没有轮次 id
// 的事件（例如某些工具不携带 turn_id）永远不算过期。
fn turn_is_stale(session: &HookSessionState, incoming_turn_id: Option<&str>) -> bool {
    incoming_turn_id.is_some_and(|incoming| {
        session
            .turn_id
            .as_deref()
            .is_some_and(|current| current != incoming)
    })
}

// 对已停止轮次收到的“可作为工作起点”的事件分类：同一 turn 只能是迟到事件，
// 不同 turn 则是 Goal 模式恢复或自动续跑产生的新隐式轮次。
fn stopped_turn_decision(
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

// 把聚合状态的变化转换成应用层需要执行的动作：状态未变则忽略，
// 变化了才转换为对应的展示/释放转换。
fn phase_decision(previous: HookPhase, next: HookPhase) -> HookEventDecision {
    if previous == next {
        return HookEventDecision::Ignore;
    }
    HookEventDecision::Forward(match next {
        HookPhase::Released => HookTransition::Release,
        HookPhase::Idle => HookTransition::Display(HookBehavior::Idle),
        HookPhase::Running => HookTransition::Display(HookBehavior::Running),
        HookPhase::Asking => HookTransition::Display(HookBehavior::Asking),
        HookPhase::Error => HookTransition::Display(HookBehavior::Error),
    })
}
