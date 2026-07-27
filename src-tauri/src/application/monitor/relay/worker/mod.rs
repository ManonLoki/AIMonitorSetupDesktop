// Hook 事件的状态机 worker 与设备投递 worker：从 axum handler
// （`relay::listener`）送来的原始事件推进各工具独立的生命周期状态机，
// 再用 latest-wins mailbox 把需要转发的最新状态交给每个工具专属的投递线程
// （实际的设备 HTTP 转发逻辑见 `relay::forward`）。
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, RwLock, mpsc},
    thread,
    time::{Duration, Instant},
};

use super::{
    forward::relay_hook_with_accounting,
    status::{record_hook_results, record_relay_failure, record_suppressed_hook},
};
use crate::{
    application::monitor::{
        HOOK_RELAY_WAKE_QUEUE_CAPACITY, HOOK_SESSION_INACTIVITY_TIMEOUT,
        HOOK_SESSION_SWEEP_INTERVAL, HookRelayStatus, IncomingHookEvent,
    },
    domain::monitor::{
        AiTool, DiscoveredMonitorDevice, HookEventDecision, HookStateMachine, HookTransition,
        SavedMonitorData, forwards_every_event,
    },
};

#[cfg(test)]
mod tests;

pub(super) struct PendingHookRelay {
    pub(super) tool: AiTool,
    pub(super) hook_type: String,
    pub(super) transition: HookTransition,
    pub(super) counts_as_hook: bool,
}

pub(super) type PendingHookRelays = Arc<Mutex<HashMap<AiTool, VecDeque<PendingHookRelay>>>>;
type HookRelayWakeSenders = HashMap<AiTool, mpsc::SyncSender<()>>;

// 启动状态机线程与其背后的每工具投递 worker：从 `receiver` 收到的原始事件
// 先推进状态机，产出的转发/超时决定再交给 `enqueue_latest_hook_relay` 排队。
pub(crate) fn spawn_hook_worker(
    client: &reqwest::blocking::Client,
    receiver: mpsc::Receiver<IncomingHookEvent>,
    data: &Arc<RwLock<SavedMonitorData>>,
    online_devices: &Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
    status: Arc<RwLock<HookRelayStatus>>,
) {
    // 网络投递使用 latest-wins mailbox：每个工具至多一个正在发送的状态和一个
    // 尚未发送的最新状态。旧的待发送中间态会被覆盖，但所有原始事件仍先按序推进
    // 状态机，因此 Stop/SessionEnd 等时序屏障不会被跳过。
    let pending_relays = Arc::new(Mutex::new(
        HashMap::<AiTool, VecDeque<PendingHookRelay>>::new(),
    ));
    let relay_wake_senders =
        spawn_hook_delivery_workers(client, &pending_relays, data, online_devices, &status);

    thread::spawn(move || {
        // 每个工具拥有独立生命周期状态机。状态机线程只执行纯内存计算，不等待
        // 设备网络，因此有界 ingress 队列在正常洪峰下也能快速被消费。
        let mut state_machines = HashMap::<AiTool, HookStateMachine>::new();
        let clock_started_at = Instant::now();
        let mut last_sweep_at = Instant::now();

        loop {
            match receiver.recv_timeout(HOOK_SESSION_SWEEP_INTERVAL) {
                Ok(event) => {
                    let observed_at = clock_started_at.elapsed();
                    // 持续有流量时 recv_timeout 不会进入 Timeout 分支，所以仍需按
                    // 固定粒度主动清扫，确保洪峰本身不能阻止会话过期。
                    if last_sweep_at.elapsed() >= HOOK_SESSION_SWEEP_INTERVAL {
                        expire_inactive_hook_sessions(
                            &mut state_machines,
                            observed_at,
                            &pending_relays,
                            &relay_wake_senders,
                            &status,
                        );
                        last_sweep_at = Instant::now();
                    }

                    let IncomingHookEvent {
                        tool,
                        hook_type,
                        session_id,
                        turn_id,
                        status: event_status,
                    } = event;
                    let decision = state_machines
                        .entry(tool)
                        .or_default()
                        .apply_event_with_status_at(
                            tool,
                            &hook_type,
                            session_id.as_deref(),
                            turn_id.as_deref(),
                            event_status.as_deref(),
                            observed_at,
                        );
                    match decision {
                        HookEventDecision::Forward(transition) => enqueue_latest_hook_relay(
                            &pending_relays,
                            &relay_wake_senders,
                            &status,
                            PendingHookRelay {
                                tool,
                                hook_type,
                                transition,
                                counts_as_hook: true,
                            },
                        ),
                        HookEventDecision::Ignore => {
                            record_suppressed_hook(&status, tool, &hook_type);
                        }
                        HookEventDecision::Unsupported => record_hook_results(
                            &status,
                            tool,
                            &hook_type,
                            None,
                            0,
                            &[format!("不支持的 Hook 类型：{hook_type}")],
                        ),
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    expire_inactive_hook_sessions(
                        &mut state_machines,
                        clock_started_at.elapsed(),
                        &pending_relays,
                        &relay_wake_senders,
                        &status,
                    );
                    last_sweep_at = Instant::now();
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

// 为每个 AI 工具各起一个独立的投递 worker 与唤醒通道，返回这些通道供
// enqueue/expire 逻辑按工具寻址唤醒，实现跨工具互不阻塞的并行投递。
fn spawn_hook_delivery_workers(
    client: &reqwest::blocking::Client,
    pending_relays: &PendingHookRelays,
    data: &Arc<RwLock<SavedMonitorData>>,
    online_devices: &Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
    status: &Arc<RwLock<HookRelayStatus>>,
) -> HookRelayWakeSenders {
    let mut wake_senders = HashMap::with_capacity(AiTool::ALL.len());
    for tool in AiTool::ALL {
        let (sender, receiver) = mpsc::sync_channel::<()>(HOOK_RELAY_WAKE_QUEUE_CAPACITY);
        spawn_hook_delivery_worker(
            tool,
            client.clone(),
            receiver,
            Arc::clone(pending_relays),
            Arc::clone(data),
            Arc::clone(online_devices),
            Arc::clone(status),
        );
        wake_senders.insert(tool, sender);
    }
    wake_senders
}

// 单个工具的投递 worker：每次被唤醒后持续从该工具的队列头部取出待投递项，
// 逐个转发给设备，直到队列清空才重新阻塞等待下一次唤醒。
fn spawn_hook_delivery_worker(
    tool: AiTool,
    client: reqwest::blocking::Client,
    receiver: mpsc::Receiver<()>,
    pending_relays: PendingHookRelays,
    data: Arc<RwLock<SavedMonitorData>>,
    online_devices: Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
    status: Arc<RwLock<HookRelayStatus>>,
) {
    thread::spawn(move || {
        while receiver.recv().is_ok() {
            loop {
                let pending = pending_relays.lock().ok().and_then(|mut pending| {
                    let queue = pending.get_mut(&tool)?;
                    let relay = queue.pop_front();
                    if queue.is_empty() {
                        pending.remove(&tool);
                    }
                    relay
                });
                let Some(pending) = pending else {
                    break;
                };
                relay_hook_with_accounting(&client, &data, &online_devices, &status, &pending);
            }
        }
    });
}

// 把一个待转发状态放进对应工具的队列，并在队列由空变为非空时唤醒投递
// worker。两种排队策略二选一（按 `forwards_every_event` 判断）：
// - 未经状态机验证的工具：整队直通，每个事件都单独入队，不做任何合并；
// - 经状态机验证的四个工具：沿用 latest-wins 语义，新状态只替换队尾（保留
//   投递 worker 正在处理的队首），旧的队尾状态被覆盖时仍需完成记账。
fn enqueue_latest_hook_relay(
    pending_relays: &PendingHookRelays,
    wake_senders: &HookRelayWakeSenders,
    status: &Arc<RwLock<HookRelayStatus>>,
    relay: PendingHookRelay,
) {
    let tool = relay.tool;
    let (should_wake, displaced) = if let Ok(mut pending) = pending_relays.lock() {
        let queue = pending.entry(tool).or_default();
        let should_wake = queue.is_empty();
        let displaced = if forwards_every_event(tool) {
            // 直通模式：不合并，直接追加到队尾。
            queue.push_back(relay);
            None
        } else {
            // latest-wins 模式：弹出旧队尾（若存在）后压入新状态。
            let displaced = queue.pop_back();
            queue.push_back(relay);
            displaced
        };
        (should_wake, displaced)
    } else {
        record_relay_failure(status, "Hook 最新状态队列不可用".to_owned());
        return;
    };

    // 被覆盖的真实 Hook 已经不需要设备投递，但仍必须完成其 pending/received
    // 记账；把它计入 suppressed 可让工作台准确反映 latest-wins 的合并次数。
    if let Some(displaced) = displaced
        && displaced.counts_as_hook
    {
        record_suppressed_hook(status, displaced.tool, &displaced.hook_type);
    }

    let Some(wake_sender) = wake_senders.get(&tool) else {
        let dropped = pending_relays
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&tool))
            .and_then(|mut queue| queue.pop_back());
        if let Some(dropped) = dropped {
            if dropped.counts_as_hook {
                record_hook_results(
                    status,
                    dropped.tool,
                    &dropped.hook_type,
                    None,
                    0,
                    &["Hook 工具投递 worker 未启动".to_owned()],
                );
            } else {
                record_relay_failure(status, "Hook 工具投递 worker 未启动".to_owned());
            }
        }
        return;
    };

    if should_wake && wake_sender.send(()).is_err() {
        let dropped = pending_relays
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&tool))
            .and_then(|mut queue| queue.pop_back());
        if let Some(dropped) = dropped {
            if dropped.counts_as_hook {
                record_hook_results(
                    status,
                    dropped.tool,
                    &dropped.hook_type,
                    None,
                    0,
                    &["Hook 设备投递线程已停止".to_owned()],
                );
            } else {
                record_relay_failure(status, "Hook 设备投递线程已停止".to_owned());
            }
        }
    }
}

// 扫描所有工具的状态机，回收长时间无事件的孤儿会话；每个产生的内部释放
// 转换都按普通事件一样入队投递（`counts_as_hook: false`，不计入收到数）。
fn expire_inactive_hook_sessions(
    state_machines: &mut HashMap<AiTool, HookStateMachine>,
    observed_at: Duration,
    pending_relays: &PendingHookRelays,
    wake_senders: &HookRelayWakeSenders,
    status: &Arc<RwLock<HookRelayStatus>>,
) {
    for (&tool, machine) in state_machines.iter_mut() {
        if let HookEventDecision::Forward(transition) =
            machine.expire_inactive_sessions(observed_at, HOOK_SESSION_INACTIVITY_TIMEOUT)
        {
            enqueue_latest_hook_relay(
                pending_relays,
                wake_senders,
                status,
                PendingHookRelay {
                    tool,
                    hook_type: "SessionTimeout".to_owned(),
                    transition,
                    counts_as_hook: false,
                },
            );
        }
    }
}
