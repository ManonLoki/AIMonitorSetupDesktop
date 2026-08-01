// Hook 事件的状态机 worker：从 listener 收到事件后推进各工具生命周期，
// 再把状态交给目标级投递调度器（见 `relay::delivery`）。
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, watch};

use super::{
    delivery::DeliveryScheduler,
    status::{record_hook_results, record_relay_failure, record_suppressed_hook},
};
#[cfg(test)]
use crate::application::monitor::HookRelayLastEvent;
use crate::{
    application::monitor::{
        HOOK_SESSION_INACTIVITY_TIMEOUT, HOOK_SESSION_SWEEP_INTERVAL, HookRelayStatus,
        IncomingHookEvent,
    },
    domain::monitor::{
        AiTool, DiscoveredMonitorDevice, HookEventDecision, HookStateMachine, HookTransition,
        SavedMonitorData, forwards_every_event,
    },
};

#[cfg(test)]
mod tests;

const DEVICE_ONLINE_REPLAY_HOOK_TYPE: &str = "DeviceOnlineReplay";

type OnlineDeviceRoutes = HashMap<String, (String, String)>;

enum HookWorkerWake {
    Event(Option<IncomingHookEvent>),
    DeviceSnapshot(Result<(), watch::error::RecvError>),
}

pub(super) struct PendingHookRelay {
    pub(super) tool: AiTool,
    pub(super) hook_type: String,
    pub(super) transition: HookTransition,
    pub(super) counts_as_hook: bool,
}

// 启动状态机 task 与目标级投递调度器：原始事件先推进每工具状态机，产生的
// 状态再按“设备 + 工具”隔离投递，慢设备不会阻塞其他设备。
pub(crate) fn spawn_hook_worker(
    client: &reqwest::blocking::Client,
    mut receiver: mpsc::Receiver<IncomingHookEvent>,
    mut device_snapshot_changes: watch::Receiver<u64>,
    data: &Arc<RwLock<SavedMonitorData>>,
    online_devices: &Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
    status: Arc<RwLock<HookRelayStatus>>,
) {
    let mut delivery = DeliveryScheduler::new(client, data, online_devices, Arc::clone(&status));
    let online_devices = Arc::clone(online_devices);

    tauri::async_runtime::spawn(async move {
        // 每个工具拥有独立生命周期状态机。状态机线程只执行纯内存计算，不等待
        // 设备网络，因此有界 ingress 队列在正常洪峰下也能快速被消费。
        let mut state_machines = HashMap::<AiTool, HookStateMachine>::new();
        let clock_started_at = Instant::now();
        let mut last_sweep_at = Instant::now();
        let mut device_snapshot_channel_open = true;
        let mut known_online_routes =
            online_device_routes(&online_devices).unwrap_or_else(|error| {
                record_relay_failure(&status, error);
                OnlineDeviceRoutes::new()
            });

        loop {
            let wake = if device_snapshot_channel_open {
                tokio::time::timeout(HOOK_SESSION_SWEEP_INTERVAL, async {
                    tokio::select! {
                        event = receiver.recv() => HookWorkerWake::Event(event),
                        changed = device_snapshot_changes.changed() => {
                            HookWorkerWake::DeviceSnapshot(changed)
                        }
                    }
                })
                .await
            } else {
                tokio::time::timeout(HOOK_SESSION_SWEEP_INTERVAL, async {
                    HookWorkerWake::Event(receiver.recv().await)
                })
                .await
            };
            match wake {
                Ok(HookWorkerWake::Event(Some(event))) => {
                    let observed_at = clock_started_at.elapsed();
                    // 持续有流量时超时分支不会触发，所以仍需按固定粒度主动
                    // 清扫，确保洪峰本身不能阻止会话过期。
                    if last_sweep_at.elapsed() >= HOOK_SESSION_SWEEP_INTERVAL {
                        expire_inactive_hook_sessions(
                            &mut state_machines,
                            observed_at,
                            &mut delivery,
                        );
                        last_sweep_at = Instant::now();
                    }

                    process_hook_event(
                        event,
                        observed_at,
                        &mut state_machines,
                        &mut delivery,
                        &status,
                    );
                }
                Ok(HookWorkerWake::Event(None)) => break,
                Ok(HookWorkerWake::DeviceSnapshot(Ok(()))) => {
                    replay_current_states_for_online_changes(
                        &state_machines,
                        &online_devices,
                        &mut known_online_routes,
                        &mut delivery,
                        &status,
                    );
                }
                Ok(HookWorkerWake::DeviceSnapshot(Err(_closed))) => {
                    device_snapshot_channel_open = false;
                }
                Err(_elapsed) => {
                    expire_inactive_hook_sessions(
                        &mut state_machines,
                        clock_started_at.elapsed(),
                        &mut delivery,
                    );
                    last_sweep_at = Instant::now();
                }
            }
        }
    });
}

fn process_hook_event(
    event: IncomingHookEvent,
    observed_at: Duration,
    state_machines: &mut HashMap<AiTool, HookStateMachine>,
    delivery: &mut DeliveryScheduler,
    status: &Arc<RwLock<HookRelayStatus>>,
) {
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
        HookEventDecision::Forward(transition) => {
            delivery.enqueue(&PendingHookRelay {
                tool,
                hook_type,
                transition,
                counts_as_hook: true,
            });
        }
        HookEventDecision::Ignore => record_suppressed_hook(status, tool, &hook_type),
        HookEventDecision::Unsupported => record_hook_results(
            status,
            tool,
            &hook_type,
            None,
            0,
            &[format!("不支持的 Hook 类型：{hook_type}")],
        ),
    }
}

fn replay_current_states_for_online_changes(
    state_machines: &HashMap<AiTool, HookStateMachine>,
    online_devices: &Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
    known_online_routes: &mut OnlineDeviceRoutes,
    delivery: &mut DeliveryScheduler,
    status: &Arc<RwLock<HookRelayStatus>>,
) {
    let current_routes = match online_device_routes(online_devices) {
        Ok(routes) => routes,
        Err(error) => {
            record_relay_failure(status, error);
            return;
        }
    };
    let replay_device_ids = device_ids_requiring_replay(known_online_routes, &current_routes);
    *known_online_routes = current_routes;

    if replay_device_ids.is_empty() {
        return;
    }
    for (&tool, machine) in state_machines {
        if forwards_every_event(tool) {
            continue;
        }
        let Some(transition) = machine.current_display_transition() else {
            continue;
        };
        delivery.enqueue_to_devices(
            &PendingHookRelay {
                tool,
                hook_type: DEVICE_ONLINE_REPLAY_HOOK_TYPE.to_owned(),
                transition,
                counts_as_hook: false,
            },
            &replay_device_ids,
        );
    }
}

fn device_ids_requiring_replay(
    previous: &OnlineDeviceRoutes,
    current: &OnlineDeviceRoutes,
) -> Vec<String> {
    let mut device_ids = current
        .iter()
        .filter(|(device_id, route)| previous.get(*device_id) != Some(*route))
        .map(|(device_id, _)| device_id.clone())
        .collect::<Vec<_>>();
    device_ids.sort();
    device_ids
}

fn online_device_routes(
    online_devices: &Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
) -> Result<OnlineDeviceRoutes, String> {
    online_devices
        .read()
        .map(|devices| {
            devices
                .iter()
                .map(|device| {
                    (
                        device.id.clone(),
                        (device.base_url.clone(), device.path.clone()),
                    )
                })
                .collect()
        })
        .map_err(|_| "在线设备读取锁已损坏，无法重放 Hook 状态".to_owned())
}

// 扫描所有工具的状态机，回收长时间无事件的孤儿会话；每个产生的内部释放
// 转换都按普通事件一样入队投递（`counts_as_hook: false`，不计入收到数）。
fn expire_inactive_hook_sessions(
    state_machines: &mut HashMap<AiTool, HookStateMachine>,
    observed_at: Duration,
    delivery: &mut DeliveryScheduler,
) {
    for (&tool, machine) in state_machines.iter_mut() {
        if let HookEventDecision::Forward(transition) =
            machine.expire_inactive_sessions(observed_at, HOOK_SESSION_INACTIVITY_TIMEOUT)
        {
            delivery.enqueue(&PendingHookRelay {
                tool,
                hook_type: "SessionTimeout".to_owned(),
                transition,
                counts_as_hook: false,
            });
        }
    }
}
