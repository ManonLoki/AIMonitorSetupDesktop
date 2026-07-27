// 把一个已通过去抖判定的 Hook 事件实际转发给设备：转换成展示/释放行为，
// 并发 POST/DELETE 到每台当前在线且配置了该 AI 工具的设备。
use std::sync::{Arc, RwLock};

#[cfg(test)]
use std::thread;

use serde::Serialize;

#[cfg(test)]
use super::{status::record_hook_results_with_accounting, worker::PendingHookRelay};
#[cfg(test)]
use crate::application::monitor::HookRelayStatus;
use crate::{
    application::monitor::device_response::ensure_success_blocking,
    domain::monitor::{
        AiProfile, AiTool, DiscoveredMonitorDevice, HookBehavior, HookTransition,
        MonitorDeviceRoute, SavedMonitorData, ai_tool_name,
    },
};

// 向设备端上报槽位状态更新时使用的请求体。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SlotUpdateRequest<'a> {
    client_id: &'a str,
    username: &'a str,
    ai_name: &'static str,
    behavior: HookBehavior,
    content: &'a str,
    image: &'a str,
}

// 返回指定工具的 Profile 总数，以及其中当前在线的设备 ID。投递调度只为
// 在线目标建立“设备 + 工具”独立队列；保存过但已离线的设备不会进入转发链路。
pub(super) fn configured_online_target_ids(
    data: &Arc<RwLock<SavedMonitorData>>,
    online_devices: &Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
    tool: AiTool,
) -> Result<(usize, Vec<String>), String> {
    let online_devices = online_devices
        .read()
        .map_err(|_| "在线设备读取锁已损坏".to_owned())?;
    let data = data.read().map_err(|_| "转发配置读取锁已损坏".to_owned())?;
    let configured = data
        .profiles
        .iter()
        .filter(|profile| profile.tool == tool)
        .collect::<Vec<_>>();
    let online_target_ids = configured
        .iter()
        .filter(|profile| {
            online_devices
                .iter()
                .any(|device| device.id == profile.device_id)
        })
        .map(|profile| profile.device_id.clone())
        .collect();
    Ok((configured.len(), online_target_ids))
}

// 只向一台目标设备投递状态。设备路由和 Profile 在真正发送前读取最新快照，
// 因而队列等待期间发生的地址发现或配置修改仍会生效。
pub(super) fn forward_hook_to_target(
    client: &reqwest::blocking::Client,
    data: &Arc<RwLock<SavedMonitorData>>,
    online_devices: &Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
    tool: AiTool,
    transition: HookTransition,
    device_id: &str,
) -> Result<bool, String> {
    // 队列等待期间设备可能离线；真正构造 HTTP 请求前再次读取在线快照。
    // 不在线属于正常跳过，不计转发成功，也不计网络失败。
    let online_device = online_devices
        .read()
        .map_err(|_| "在线设备读取锁已损坏".to_owned())?
        .iter()
        .find(|device| device.id == device_id)
        .cloned();
    let Some(online_device) = online_device else {
        return Ok(false);
    };
    let data = data.read().map_err(|_| "转发配置读取锁已损坏".to_owned())?;
    let username = data.settings.username.clone();
    let client_id = data.client_id.clone();
    let saved_device = data
        .devices
        .iter()
        .find(|device| device.device_id == device_id)
        .cloned()
        .ok_or_else(|| format!("目标设备 {device_id} 不存在"))?;
    let profile = data
        .profiles
        .iter()
        .find(|profile| profile.device_id == device_id && profile.tool == tool)
        .cloned()
        .ok_or_else(|| format!("目标设备 {} 的 AI Profile 不存在", saved_device.device_name))?;
    drop(data);

    let effective_device = MonitorDeviceRoute {
        base_url: online_device.base_url,
        device_id: online_device.id,
        device_name: online_device.name,
    };
    forward_profile(
        client,
        tool,
        transition,
        &username,
        &client_id,
        &effective_device,
        &profile,
    )
    .map(|()| true)
    .map_err(|error| format!("{}：{error}", effective_device.device_name))
}

#[cfg(test)]
mod tests;

// 处理一个已通过去抖判定的 Hook 事件：转换为业务行为、找出所有配置了该工具的
// 在线优先设备，并发转发过去，最终把结果（成功数/错误列表）记录进中继状态。
#[cfg(test)]
fn relay_hook(
    client: &reqwest::blocking::Client,
    data: &Arc<RwLock<SavedMonitorData>>,
    online_devices: &Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
    status: &Arc<RwLock<HookRelayStatus>>,
    tool: AiTool,
    hook_type: &str,
    transition: HookTransition,
) {
    let pending = PendingHookRelay {
        tool,
        hook_type: hook_type.to_owned(),
        transition,
        counts_as_hook: true,
    };
    relay_hook_with_accounting(client, data, online_devices, status, &pending);
}

// 处理一个已通过去抖判定的 Hook 事件：只找出当前在线且配置了该工具的设备，
// 并发转发，最终把结果（成功数/错误列表）记录进中继状态。
#[cfg(test)]
fn relay_hook_with_accounting(
    client: &reqwest::blocking::Client,
    data: &Arc<RwLock<SavedMonitorData>>,
    online_devices: &Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
    status: &Arc<RwLock<HookRelayStatus>>,
    pending: &PendingHookRelay,
) {
    let tool = pending.tool;
    let hook_type = pending.hook_type.as_str();
    let transition = pending.transition;
    let counts_as_hook = pending.counts_as_hook;
    // 读取共享配置数据；锁损坏则记录失败并返回。
    let Ok(data) = data.read() else {
        record_hook_results_with_accounting(
            status,
            tool,
            hook_type,
            None,
            0,
            &["转发配置读取锁已损坏".to_owned()],
            counts_as_hook,
        );
        return;
    };
    // 克隆出一份快照后立即释放读锁，避免长时间持锁阻塞其他操作。
    let snapshot = data.clone();
    drop(data);
    // 读取当前在线设备快照（读取失败则视为没有在线设备）。
    let online_snapshot = online_devices
        .read()
        .map(|devices| devices.clone())
        .unwrap_or_default();
    // 找出当前在线且已配置该 AI 工具展示 Profile 的设备（与 Profile 配对）。
    let targets = snapshot
        .profiles
        .iter()
        .filter(|profile| profile.tool == tool)
        .filter(|profile| {
            online_snapshot
                .iter()
                .any(|device| device.id == profile.device_id)
        })
        .filter_map(|profile| {
            snapshot
                .devices
                .iter()
                .find(|device| device.device_id == profile.device_id)
                .map(|device| (device, profile))
        })
        .collect::<Vec<_>>();
    // 没有任何在线目标时正常记下已接收事件，但不尝试历史路由。
    if targets.is_empty() {
        record_hook_results_with_accounting(
            status,
            tool,
            hook_type,
            match transition {
                HookTransition::Display(behavior) => Some(behavior),
                HookTransition::Release => None,
            },
            0,
            &[],
            counts_as_hook,
        );
        return;
    }

    // Display 转换携带具体行为类型；Release（释放/清空展示）没有行为类型。
    let behavior = match transition {
        HookTransition::Display(behavior) => Some(behavior),
        HookTransition::Release => None,
    };
    // 并发转发给所有目标设备，汇总成功次数与错误信息列表。
    let (forwarded, errors) = forward_to_all_targets(
        client,
        tool,
        transition,
        &snapshot.settings.username,
        &snapshot.client_id,
        targets,
        &online_snapshot,
    );
    // 把本次转发的结果写入中继状态，供前端查询展示。
    record_hook_results_with_accounting(
        status,
        tool,
        hook_type,
        behavior,
        forwarded,
        &errors,
        counts_as_hook,
    );
}

/// 并发转发给每台已配置该 AI 的设备：先一次性 spawn 所有目标的转发线程，
/// 再统一 join，这样单台设备网络慢或不可达时不会拖慢其余设备收到状态更新的
/// 时间；每台设备的成功/失败互不影响。
#[cfg(test)]
fn forward_to_all_targets(
    client: &reqwest::blocking::Client,
    tool: AiTool,
    transition: HookTransition,
    username: &str,
    client_id: &str,
    targets: Vec<(&MonitorDeviceRoute, &AiProfile)>,
    online_snapshot: &[DiscoveredMonitorDevice],
) -> (u64, Vec<String>) {
    let outcomes = thread::scope(|scope| {
        targets
            .into_iter()
            .map(|(saved_device, profile)| {
                // targets 已经过在线过滤，这里必须能取到发现快照中的最新地址。
                let online_device = online_snapshot
                    .iter()
                    .find(|device| device.id == saved_device.device_id)
                    .expect("online target must exist in the discovery snapshot");
                let effective_device = MonitorDeviceRoute {
                    base_url: online_device.base_url.clone(),
                    device_id: online_device.id.clone(),
                    device_name: online_device.name.clone(),
                };
                // 为每个目标设备单独开一个线程并发转发，互不阻塞。
                scope.spawn(move || {
                    let result = forward_profile(
                        client,
                        tool,
                        transition,
                        username,
                        client_id,
                        &effective_device,
                        profile,
                    );
                    (effective_device.device_name, result)
                })
            })
            // 先收集成 Vec 触发所有线程实际 spawn（避免惰性求值导致串行执行）。
            .collect::<Vec<_>>()
            .into_iter()
            // 再统一 join 等待每个线程结果；线程 panic 时转换为错误而不是让调用方 panic。
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| ("未知设备".to_owned(), Err("转发线程异常终止".to_owned())))
            })
            .collect::<Vec<_>>()
    });

    // 汇总所有设备的转发结果：成功计数与失败设备的错误信息列表。
    let mut forwarded = 0_u64;
    let mut errors = Vec::new();
    for (device_name, result) in outcomes {
        match result {
            Ok(()) => forwarded += 1,
            Err(error) => errors.push(format!("{device_name}：{error}")),
        }
    }
    (forwarded, errors)
}

// 把单个 AI 工具的状态转换（展示或释放）实际发送给单台设备的指定槽位。
fn forward_profile(
    client: &reqwest::blocking::Client,
    tool: AiTool,
    transition: HookTransition,
    username: &str,
    client_id: &str,
    device: &MonitorDeviceRoute,
    profile: &AiProfile,
) -> Result<(), String> {
    // 设备地址或用户名为空则无法发送，直接返回错误。
    if device.base_url.is_empty() || username.is_empty() {
        return Err("设备地址或显示用户名为空".to_owned());
    }
    let url = format!("{}/api/slots/{}", device.base_url, profile.slot);
    match transition {
        // 展示行为：在 Profile 里找到该行为对应的展示内容（文案+图片），POST 给设备。
        HookTransition::Display(behavior) => profile
            .hooks
            .iter()
            .find(|state| state.behavior == behavior)
            .ok_or_else(|| "AI 状态配置不完整".to_owned())
            .and_then(|state| {
                send_and_confirm(
                    client.post(&url).json(&SlotUpdateRequest {
                        client_id,
                        username,
                        ai_name: ai_tool_name(tool),
                        behavior,
                        content: &state.content,
                        image: &state.image,
                    }),
                    "转发到监控屏失败",
                    "监控屏拒绝了状态更新",
                )
            }),
        // 释放行为：直接对该槽位发 DELETE 请求清空展示。
        HookTransition::Release => send_and_confirm(
            client.delete(&url),
            "释放监控屏位置失败",
            "监控屏拒绝了位置释放",
        ),
    }
}

/// 发送请求并确认设备接受：`send_label` 用于网络层失败（连不上/超时），
/// `reject_label` 用于设备返回非成功状态（连上了但拒绝了这次操作）。
fn send_and_confirm(
    request: reqwest::blocking::RequestBuilder,
    send_label: &str,
    reject_label: &str,
) -> Result<(), String> {
    request
        .send()
        // 网络层失败（连不上/超时等）用 send_label 包装错误信息。
        .map_err(|error| format!("{send_label}：{error}"))
        .and_then(|response| {
            // 收到响应但设备拒绝（非 2xx）用 reject_label 包装错误信息。
            ensure_success_blocking(response)
                .map(|_| ())
                .map_err(|error| format!("{reject_label}：{error}"))
        })
}
