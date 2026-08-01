// Hook 状态的目标级投递调度：每个“设备 + AI 工具”拥有独立 worker 和队列。
// 慢设备只会阻塞自身后续状态，不再拖住同一工具在其他在线设备上的更新。
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use super::{
    forward::{configured_online_target_ids, forward_hook_to_target},
    status::{
        record_hook_results, record_hook_results_with_accounting, record_partial_suppression,
        record_relay_failure, record_suppressed_hook,
    },
    worker::PendingHookRelay,
};
use crate::{
    application::monitor::{HOOK_RELAY_WAKE_QUEUE_CAPACITY, HookRelayStatus},
    domain::monitor::{
        AiTool, DiscoveredMonitorDevice, HookTransition, SavedMonitorData, forwards_every_event,
        release_settle_delay,
    },
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DeliveryKey {
    tool: AiTool,
    device_id: String,
}

struct PendingTargetRelay {
    transition: HookTransition,
    tracker: Arc<DeliveryTracker>,
}

type PendingTargetRelays = Arc<Mutex<HashMap<DeliveryKey, VecDeque<PendingTargetRelay>>>>;

// 一次 Hook 事件可能面向多台设备。各目标独立完成后由 tracker 只记一次
// received/pending，并聚合成功、失败和目标级时序抑制结果。
struct DeliveryTracker {
    status: Arc<RwLock<HookRelayStatus>>,
    tool: AiTool,
    hook_type: String,
    transition: HookTransition,
    counts_as_hook: bool,
    target_count: usize,
    remaining: AtomicUsize,
    delivery_outcomes: AtomicUsize,
    forwarded: AtomicU64,
    suppressed: AtomicUsize,
    errors: Mutex<Vec<String>>,
}

impl DeliveryTracker {
    fn new(
        status: Arc<RwLock<HookRelayStatus>>,
        relay: &PendingHookRelay,
        target_count: usize,
    ) -> Arc<Self> {
        Arc::new(Self {
            status,
            tool: relay.tool,
            hook_type: relay.hook_type.clone(),
            transition: relay.transition,
            counts_as_hook: relay.counts_as_hook,
            target_count,
            remaining: AtomicUsize::new(target_count),
            delivery_outcomes: AtomicUsize::new(0),
            forwarded: AtomicU64::new(0),
            suppressed: AtomicUsize::new(0),
            errors: Mutex::new(Vec::new()),
        })
    }

    fn delivered(&self, result: Result<bool, String>) {
        self.delivery_outcomes.fetch_add(1, Ordering::Relaxed);
        match result {
            Ok(true) => {
                self.forwarded.fetch_add(1, Ordering::Relaxed);
            }
            Ok(false) => {}
            Err(error) => self.push_error(error),
        }
        self.finish_target();
    }

    // 队列或 worker 调度在目标真正处理 transition 前失败。这类失败
    // 仍需记账，但不能把 Release 写成最近业务事件。
    fn failed_before_delivery(&self, error: String) {
        self.push_error(error);
        self.finish_target();
    }

    fn push_error(&self, error: String) {
        if let Ok(mut errors) = self.errors.lock() {
            errors.push(error);
        }
    }

    fn suppressed(&self) {
        self.suppressed.fetch_add(1, Ordering::Relaxed);
        self.finish_target();
    }

    fn finish_target(&self) {
        if self.remaining.fetch_sub(1, Ordering::AcqRel) != 1 {
            return;
        }
        let suppressed = self.suppressed.load(Ordering::Acquire);
        if suppressed == self.target_count {
            if self.counts_as_hook {
                record_suppressed_hook(&self.status, self.tool, &self.hook_type);
            }
            return;
        }
        let errors = self.errors.lock().map_or_else(
            |_| vec!["Hook 投递结果锁已损坏".to_owned()],
            |errors| errors.clone(),
        );
        let transition =
            (self.delivery_outcomes.load(Ordering::Acquire) > 0).then_some(self.transition);
        record_hook_results_with_accounting(
            &self.status,
            self.tool,
            &self.hook_type,
            transition,
            self.forwarded.load(Ordering::Acquire),
            &errors,
            self.counts_as_hook,
        );
        if suppressed > 0 && self.counts_as_hook {
            record_partial_suppression(&self.status);
        }
    }
}

pub(super) struct DeliveryScheduler {
    client: reqwest::blocking::Client,
    data: Arc<RwLock<SavedMonitorData>>,
    online_devices: Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
    status: Arc<RwLock<HookRelayStatus>>,
    pending: PendingTargetRelays,
    wake_senders: HashMap<DeliveryKey, mpsc::SyncSender<()>>,
}

impl DeliveryScheduler {
    pub(super) fn new(
        client: &reqwest::blocking::Client,
        data: &Arc<RwLock<SavedMonitorData>>,
        online_devices: &Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
        status: Arc<RwLock<HookRelayStatus>>,
    ) -> Self {
        Self {
            client: client.clone(),
            data: Arc::clone(data),
            online_devices: Arc::clone(online_devices),
            status,
            pending: Arc::new(Mutex::new(HashMap::new())),
            wake_senders: HashMap::new(),
        }
    }

    pub(super) fn enqueue(&mut self, relay: &PendingHookRelay) {
        let target_ids =
            match configured_online_target_ids(&self.data, &self.online_devices, relay.tool) {
                Ok((_, target_ids)) if !target_ids.is_empty() => target_ids,
                Ok((0, _)) => {
                    record_hook_results_with_accounting(
                        &self.status,
                        relay.tool,
                        &relay.hook_type,
                        None,
                        0,
                        &["尚未配置该 AI 的转发位置".to_owned()],
                        relay.counts_as_hook,
                    );
                    return;
                }
                Ok((_, _)) => {
                    record_hook_results_with_accounting(
                        &self.status,
                        relay.tool,
                        &relay.hook_type,
                        Some(relay.transition),
                        0,
                        &[],
                        relay.counts_as_hook,
                    );
                    return;
                }
                Err(error) => {
                    if relay.counts_as_hook {
                        record_hook_results(
                            &self.status,
                            relay.tool,
                            &relay.hook_type,
                            None,
                            0,
                            &[error],
                        );
                    } else {
                        record_relay_failure(&self.status, error);
                    }
                    return;
                }
            };
        let tracker = DeliveryTracker::new(Arc::clone(&self.status), relay, target_ids.len());
        for device_id in target_ids {
            let key = DeliveryKey {
                tool: relay.tool,
                device_id,
            };
            self.ensure_worker(&key);
            self.enqueue_target(
                &key,
                PendingTargetRelay {
                    transition: relay.transition,
                    tracker: Arc::clone(&tracker),
                },
            );
        }
    }

    fn ensure_worker(&mut self, key: &DeliveryKey) {
        if self.wake_senders.contains_key(key) {
            return;
        }
        let (sender, receiver) = mpsc::sync_channel::<()>(HOOK_RELAY_WAKE_QUEUE_CAPACITY);
        spawn_target_worker(
            key.clone(),
            self.client.clone(),
            receiver,
            Arc::clone(&self.pending),
            Arc::clone(&self.data),
            Arc::clone(&self.online_devices),
            release_settle_delay(key.tool),
        );
        self.wake_senders.insert(key.clone(), sender);
    }

    fn enqueue_target(&self, key: &DeliveryKey, relay: PendingTargetRelay) {
        let (should_wake, displaced) = if let Ok(mut pending) = self.pending.lock() {
            let queue = pending.entry(key.clone()).or_default();
            let should_wake = queue.is_empty();
            let displaced = if forwards_every_event(key.tool) {
                queue.push_back(relay);
                None
            } else {
                let displaced = queue.pop_back();
                queue.push_back(relay);
                displaced
            };
            (should_wake, displaced)
        } else {
            relay
                .tracker
                .failed_before_delivery("Hook 目标队列不可用".to_owned());
            return;
        };
        if let Some(displaced) = displaced {
            displaced.tracker.suppressed();
        }
        let wake_failed = should_wake
            && self.wake_senders.get(key).is_none_or(|sender| {
                matches!(
                    sender.try_send(()),
                    Err(mpsc::TrySendError::Disconnected(()))
                )
            });
        if wake_failed {
            let dropped = self
                .pending
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(key))
                .unwrap_or_default();
            for dropped in dropped {
                dropped
                    .tracker
                    .failed_before_delivery("Hook 目标投递 worker 未启动".to_owned());
            }
        }
    }
}

fn spawn_target_worker(
    key: DeliveryKey,
    client: reqwest::blocking::Client,
    receiver: mpsc::Receiver<()>,
    pending: PendingTargetRelays,
    data: Arc<RwLock<SavedMonitorData>>,
    online_devices: Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
    settle_delay: Duration,
) {
    thread::spawn(move || {
        'worker: while receiver.recv().is_ok() {
            loop {
                let relay = match pending.lock() {
                    Ok(mut pending) => {
                        let Some(queue) = pending.get_mut(&key) else {
                            break;
                        };
                        let relay = queue.pop_front();
                        if queue.is_empty() {
                            pending.remove(&key);
                        }
                        relay
                    }
                    Err(poisoned) => {
                        // 不能把锁损坏伪装成空队列：否则已经入队的 tracker
                        // 永远不会完成，工作台的 pending 计数也会永久悬挂。
                        // 恢复 guard 只为失败剩余任务并退出；后续调度走 Disconnected。
                        let dropped = {
                            let mut pending = poisoned.into_inner();
                            pending.remove(&key).unwrap_or_default()
                        };
                        for dropped in dropped {
                            dropped.tracker.failed_before_delivery(
                                "Hook 目标队列不可用，目标投递 worker 已停止".to_owned(),
                            );
                        }
                        break 'worker;
                    }
                };
                let Some(relay) = relay else {
                    break;
                };
                if relay.transition == HookTransition::Release {
                    match release_has_pending_successor(&receiver, &pending, &key, settle_delay) {
                        Ok(true) => {
                            relay.tracker.suppressed();
                            continue;
                        }
                        Ok(false) => {}
                        Err(error) => {
                            relay.tracker.failed_before_delivery(error);
                            continue;
                        }
                    }
                }
                let result = forward_hook_to_target(
                    &client,
                    &data,
                    &online_devices,
                    key.tool,
                    relay.transition,
                    &key.device_id,
                );
                relay.tracker.delivered(result);
            }
        }
    });
}

// Cursor 等声明了交接缓冲的协议，在真正发送 destructive DELETE 前等待同一
// “设备 + 工具”的继任展示。唤醒令牌可能是 worker 忙于上一请求时留下的旧令牌，
// 因此必须以 pending 队列为准，并持续排空旧令牌直到 deadline。
fn release_has_pending_successor(
    receiver: &mpsc::Receiver<()>,
    pending: &PendingTargetRelays,
    key: &DeliveryKey,
    settle_delay: Duration,
) -> Result<bool, String> {
    if settle_delay.is_zero() {
        return Ok(false);
    }
    let deadline = Instant::now() + settle_delay;
    loop {
        if pending_has_successor(pending, key)? {
            return Ok(true);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        match receiver.recv_timeout(remaining) {
            Ok(()) => {}
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {
                return pending_has_successor(pending, key);
            }
        }
    }
}

fn pending_has_successor(pending: &PendingTargetRelays, key: &DeliveryKey) -> Result<bool, String> {
    let pending = pending
        .lock()
        .map_err(|_| "Hook 目标队列不可用，已拒绝发送释放请求".to_owned())?;
    Ok(pending.get(key).is_some_and(|queue| !queue.is_empty()))
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_release_settle;
