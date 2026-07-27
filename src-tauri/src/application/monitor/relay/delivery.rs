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
};

use super::{
    forward::{configured_target_ids, forward_hook_to_target},
    status::{
        record_hook_results, record_hook_results_with_accounting, record_partial_suppression,
        record_relay_failure, record_suppressed_hook,
    },
    worker::PendingHookRelay,
};
use crate::{
    application::monitor::{HOOK_RELAY_WAKE_QUEUE_CAPACITY, HookRelayStatus},
    domain::monitor::{
        AiTool, DiscoveredMonitorDevice, HookBehavior, HookTransition, SavedMonitorData,
        forwards_every_event,
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
    behavior: Option<HookBehavior>,
    counts_as_hook: bool,
    target_count: usize,
    remaining: AtomicUsize,
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
            behavior: match relay.transition {
                HookTransition::Display(behavior) => Some(behavior),
                HookTransition::Release => None,
            },
            counts_as_hook: relay.counts_as_hook,
            target_count,
            remaining: AtomicUsize::new(target_count),
            forwarded: AtomicU64::new(0),
            suppressed: AtomicUsize::new(0),
            errors: Mutex::new(Vec::new()),
        })
    }

    fn delivered(&self, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.forwarded.fetch_add(1, Ordering::Relaxed);
            }
            Err(error) => {
                if let Ok(mut errors) = self.errors.lock() {
                    errors.push(error);
                }
            }
        }
        self.finish_target();
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
        record_hook_results_with_accounting(
            &self.status,
            self.tool,
            &self.hook_type,
            self.behavior,
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
        let target_ids = match configured_target_ids(&self.data, relay.tool) {
            Ok(target_ids) if !target_ids.is_empty() => target_ids,
            Ok(_) => {
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
                .delivered(Err("Hook 目标队列不可用".to_owned()));
            return;
        };
        if let Some(displaced) = displaced {
            displaced.tracker.suppressed();
        }
        if should_wake
            && self
                .wake_senders
                .get(key)
                .is_none_or(|sender| sender.send(()).is_err())
        {
            let dropped = self
                .pending
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(key))
                .unwrap_or_default();
            for dropped in dropped {
                dropped
                    .tracker
                    .delivered(Err("Hook 目标投递 worker 未启动".to_owned()));
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
) {
    thread::spawn(move || {
        while receiver.recv().is_ok() {
            loop {
                let relay = pending.lock().ok().and_then(|mut pending| {
                    let queue = pending.get_mut(&key)?;
                    let relay = queue.pop_front();
                    if queue.is_empty() {
                        pending.remove(&key);
                    }
                    relay
                });
                let Some(relay) = relay else {
                    break;
                };
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

#[cfg(test)]
mod tests;
