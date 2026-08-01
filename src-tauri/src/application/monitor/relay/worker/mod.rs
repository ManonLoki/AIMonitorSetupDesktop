// Hook 事件的状态机 worker：从 listener 收到事件后推进各工具生命周期，
// 再把状态交给目标级投递调度器（见 `relay::delivery`）。
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};
use tokio::{
    sync::mpsc,
    time::{MissedTickBehavior, interval_at},
};

use super::{
    delivery::DeliveryScheduler,
    status::{record_hook_results, record_suppressed_hook},
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
        SavedMonitorData,
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

// 启动状态机 task 与目标级投递调度器：原始事件先推进每工具状态机，产生的
// 状态再按“设备 + 工具”隔离投递，慢设备不会阻塞其他设备。
pub(crate) fn spawn_hook_worker(
    client: &reqwest::blocking::Client,
    mut receiver: mpsc::Receiver<IncomingHookEvent>,
    data: &Arc<RwLock<SavedMonitorData>>,
    online_devices: &Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
    status: Arc<RwLock<HookRelayStatus>>,
) {
    let mut delivery = DeliveryScheduler::new(client, data, online_devices, Arc::clone(&status));

    tauri::async_runtime::spawn(async move {
        // 每个工具拥有独立生命周期状态机。状态机线程只执行纯内存计算，不等待
        // 设备网络，因此有界 ingress 队列在正常洪峰下也能快速被消费。
        let mut state_machines = HashMap::<AiTool, HookStateMachine>::new();
        let clock_started_at = Instant::now();
        let mut last_sweep_at = Instant::now();
        let first_sweep = tokio::time::Instant::now() + HOOK_SESSION_SWEEP_INTERVAL;
        let mut sweep = interval_at(first_sweep, HOOK_SESSION_SWEEP_INTERVAL);
        // 若 runtime 曾被暂停，不在恢复时连续补跑多个无意义清扫周期。
        sweep.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                event = receiver.recv() => {
                    let Some(event) = event else {
                        break;
                    };
                    let observed_at = clock_started_at.elapsed();
                    // 持续有流量时仍在事件边界检查清扫间隔，避免 ready ingress
                    // 在极端洪峰下长期压过 timer 分支。
                    if last_sweep_at.elapsed() >= HOOK_SESSION_SWEEP_INTERVAL {
                        expire_inactive_hook_sessions(
                            &mut state_machines,
                            observed_at,
                            &mut delivery,
                        );
                        last_sweep_at = Instant::now();
                        sweep.reset();
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
                        HookEventDecision::Forward(transition) => {
                            delivery.enqueue(&PendingHookRelay {
                                tool,
                                hook_type,
                                transition,
                                counts_as_hook: true,
                            });
                        }
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
                _ = sweep.tick() => {
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
