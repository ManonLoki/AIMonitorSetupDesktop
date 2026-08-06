// MonitorService 的设备发现编排：后台轮询、发布在线快照、选中设备、
// mDNS/UDP 并发发现、可达性探测。纯算法部分见 `discovery` 子模块。
use std::{
    collections::HashMap,
    thread,
    time::{Duration, Instant},
};

use mdns_sd::{ServiceDaemon, ServiceEvent};
use tauri::{AppHandle, Emitter};
use tracing::{debug, warn};

use super::{
    AIMONITOR_SERVICE_TYPE, DEFAULT_DEVICE_API_PATH, DISCOVERY_POLL_GRANULARITY, DISCOVERY_TIMEOUT,
    MONITOR_DEVICES_CHANGED_EVENT, MonitorService,
    discovery::{
        DiscoveryCandidate, candidate_url_priority, discover_udp_candidates, discovery_base_url,
        merge_discovery_candidates,
    },
};
use crate::domain::AppError;
use crate::domain::monitor::{
    DiscoveredMonitorDevice, DiscoverySource, validate_discovery_interval_minutes,
};

mod snapshot;
mod snapshot_state;

pub use snapshot::MonitorDeviceSnapshot;

#[cfg(test)]
mod concurrency_tests;
#[cfg(test)]
mod tests;

impl MonitorService {
    /// 在独立后台线程中立即发现一次设备，之后按设置页配置的间隔（默认一
    /// 分钟）持续刷新在线设备快照。循环以 1 秒粒度醒来并重新读取当前配置
    /// 的间隔，因此用户在设置页修改间隔后无需重启应用或线程即可立即生效。
    /// 只有快照实际变化时才向前端发送事件，避免无意义地重绘设备列表。
    pub fn start_background_device_discovery(&self, app: AppHandle) {
        // clone 出的 service 只是共享状态的句柄，可以安全移动进新线程。
        let service = self.clone();
        thread::spawn(move || {
            let mut next_run = Instant::now();
            loop {
                // 到达计划执行时间才真正跑一次发现，否则只是短暂休眠等待。
                if Instant::now() >= next_run {
                    // 后台轮询与前端强制刷新共用同一编排，避免两条路径产生
                    // 不同的探测、自动切换或事件负载语义。
                    if let Err(error) =
                        tauri::async_runtime::block_on(service.refresh_online_devices(&app))
                    {
                        warn!(%error, "后台设备发现刷新失败");
                    }
                    // 按当前配置的检查间隔计算下一次执行时间。
                    next_run = Instant::now() + service.discovery_interval();
                }
                // 以固定粒度醒来检查，保证配置变更后间隔能较快生效。
                thread::sleep(DISCOVERY_POLL_GRANULARITY);
            }
        });
    }

    /// 读取当前配置的自动检查间隔；配置读取失败或存了非法值时退回默认值，
    /// 保证后台发现循环始终能继续运行。
    fn discovery_interval(&self) -> Duration {
        let minutes = self
            .settings()
            .map(|settings| settings.discovery_interval_minutes)
            .ok()
            .and_then(|minutes| validate_discovery_interval_minutes(minutes).ok())
            .unwrap_or(crate::domain::monitor::DEFAULT_DISCOVERY_INTERVAL_MINUTES);
        Duration::from_secs(minutes * 60)
    }

    /// 强制执行一次完整在线设备刷新。阻塞的 mDNS/UDP 候选收集交给
    /// Tauri 阻塞线程池，然后完成可达性探测、稳定化、自动切换与快照发布。
    /// command 只需调用本方法，不再复制业务编排。
    pub async fn refresh_online_devices(
        &self,
        app: &AppHandle,
    ) -> Result<MonitorDeviceSnapshot, AppError> {
        let refresh_generation = self.begin_online_device_refresh()?;
        let candidates = tauri::async_runtime::spawn_blocking(Self::discover_device_candidates)
            .await
            .map_err(|error| {
                AppError::new("error.discovery.taskFailed").param("detail", error.to_string())
            })??;
        let devices = self.finish_device_discovery(candidates).await?;
        self.publish_online_devices(app, refresh_generation, devices)
    }

    // 将一批设备发现结果发布为最新在线快照：先做去抖稳定处理，
    // 再在必要时自动选中第一台可用设备，最后仅在快照真正变化时才广播事件。
    fn publish_online_devices(
        &self,
        app: &AppHandle,
        refresh_generation: u64,
        devices: Vec<DiscoveredMonitorDevice>,
    ) -> Result<MonitorDeviceSnapshot, AppError> {
        let (snapshot, changed) = self.update_online_devices(refresh_generation, devices)?;
        if changed {
            debug!(device_count = snapshot.devices.len(), "在线设备快照已变化");
            let _ = app.emit(MONITOR_DEVICES_CHANGED_EVENT, snapshot.clone());
        }
        Ok(snapshot)
    }

    /// 设备发现的主入口：mDNS 和 UDP 广播总是都跑一遍并按设备 id 合并结果，
    /// 而不是任一路先返回非空结果就跳过另一路——两台设备可能只有其中一台
    /// 的 mDNS 广播能穿透当前网络（多播被 AP 丢弃、跨 VLAN 等），仅在其中一路
    /// 发现了设备时就放弃另一路会把只靠 UDP 广播现身的设备从列表里漏掉。
    /// 只有两路都出错才报错。
    pub(crate) fn discover_device_candidates() -> Result<Vec<DiscoveryCandidate>, AppError> {
        // 用 thread::scope 并发跑 mDNS 发现和 UDP 广播发现两路，互不阻塞。
        let (mdns_result, udp_result) = thread::scope(|scope| {
            let mdns = scope.spawn(Self::discover_mdns_candidates);
            let udp = scope.spawn(discover_udp_candidates);
            (
                // 子线程 panic 时也转换成 Err，不让整个发现流程 panic。
                mdns.join()
                    .unwrap_or_else(|_| Err(AppError::new("error.discovery.mdnsThreadCrashed"))),
                udp.join()
                    .unwrap_or_else(|_| Err(AppError::new("error.discovery.udpThreadCrashed"))),
            )
        });
        match (mdns_result, udp_result) {
            // 两路都成功：按设备 id 合并结果。
            (Ok(mdns_candidates), Ok(udp_candidates)) => {
                Ok(merge_discovery_candidates(mdns_candidates, udp_candidates))
            }
            // 只有一路成功：直接使用成功的一路，不因另一路失败而报错。
            (Ok(candidates), Err(_)) | (Err(_), Ok(candidates)) => Ok(candidates),
            // 两路都失败才报错，并把两边的错误信息都带上。
            (Err(mdns_error), Err(udp_error)) => {
                warn!(mdns = %mdns_error, udp = %udp_error, "mDNS 与 UDP 广播发现均失败");
                Err(AppError::new("error.discovery.bothFailed")
                    .param("mdns", mdns_error.to_string())
                    .param("udp", udp_error.to_string()))
            }
        }
    }

    // 通过 mDNS 浏览 `_aimonitor._tcp.local.` 服务类型，在超时时间内收集所有被解析出的设备。
    fn discover_mdns_candidates() -> Result<Vec<DiscoveryCandidate>, AppError> {
        // 创建 mDNS 守护实例。
        let daemon = ServiceDaemon::new().map_err(|error| {
            AppError::new("error.discovery.mdnsStartFailed").param("detail", error.to_string())
        })?;
        // 缩短网卡状态刷新周期到 1 秒，尽量及时感知网络变化（如切换 Wi-Fi）。
        daemon.set_ip_check_interval(1).map_err(|error| {
            AppError::new("error.discovery.mdnsRefreshFailed").param("detail", error.to_string())
        })?;
        // 开始浏览指定服务类型，得到一个事件接收器。
        let receiver = daemon.browse(AIMONITOR_SERVICE_TYPE).map_err(|error| {
            AppError::new("error.discovery.mdnsBrowseFailed").param("detail", error.to_string())
        })?;
        let deadline = Instant::now() + DISCOVERY_TIMEOUT;
        let mut candidates: HashMap<String, DiscoveryCandidate> = HashMap::new();

        // 循环接收事件，直到超时截止时间到达。
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            match receiver.recv_timeout(remaining) {
                Ok(ServiceEvent::ServiceResolved(service)) => {
                    // 从服务的 TXT 属性中读取设备 id，缺省回退到服务全名。
                    let properties = service.get_properties();
                    let id = properties
                        .get_property_val_str("id")
                        .unwrap_or(service.get_fullname())
                        .to_owned();
                    // 读取设备显示名称，缺省回退到服务全名。
                    let name = properties
                        .get_property_val_str("name")
                        .unwrap_or(service.get_fullname())
                        .to_owned();
                    // 读取 API 版本号，缺省默认为 "1"。
                    let api_version = properties
                        .get_property_val_str("apiVersion")
                        .unwrap_or("1")
                        .to_owned();
                    // 读取设备 API 路径前缀，缺省使用默认路径常量。
                    let path = properties
                        .get_property_val_str("path")
                        .unwrap_or(DEFAULT_DEVICE_API_PATH)
                        .to_owned();
                    // 把服务解析出的所有地址转换为可用的 base url 候选列表。
                    let mut base_urls = service
                        .get_addresses()
                        .iter()
                        .filter_map(|address| discovery_base_url(address, service.get_port()))
                        .collect::<Vec<_>>();
                    // 若地址解析全部失败，退而使用主机名拼出一个候选（依赖本地 DNS/mDNS 解析）。
                    if base_urls.is_empty() {
                        let host = service.get_hostname().trim_end_matches('.');
                        if !host.is_empty() {
                            base_urls.push(format!("http://{host}:{}", service.get_port()));
                        }
                    }
                    // 按 IPv4 优先规则排序并去重。
                    base_urls.sort_by_key(|url| candidate_url_priority(url));
                    base_urls.dedup();

                    // 按设备 id 取出或新建候选记录。
                    let candidate =
                        candidates
                            .entry(id.clone())
                            .or_insert_with(|| DiscoveryCandidate {
                                device: DiscoveredMonitorDevice {
                                    id,
                                    name: name.clone(),
                                    api_version: api_version.clone(),
                                    base_url: base_urls.first().cloned().unwrap_or_default(),
                                    path: path.clone(),
                                    discovery_source: DiscoverySource::Mdns,
                                },
                                base_urls: Vec::new(),
                            });
                    // 每次收到新的解析事件都刷新设备的名称/版本/路径（可能会变化）。
                    candidate.device.name = name;
                    candidate.device.api_version = api_version;
                    candidate.device.path = path;
                    // 合并新解析出的地址，再排序去重。
                    candidate.base_urls.extend(base_urls);
                    candidate
                        .base_urls
                        .sort_by_key(|url| candidate_url_priority(url));
                    candidate.base_urls.dedup();
                    // 用排序后第一个候选地址更新设备的主 base_url。
                    if let Some(base_url) = candidate.base_urls.first() {
                        candidate.device.base_url.clone_from(base_url);
                    }
                }
                // 其他类型事件（如服务下线、搜索开始等）忽略不处理。
                Ok(_) => {}
                // 接收超时或通道关闭，跳出循环结束发现。
                Err(_) => break,
            }
        }

        // 收尾：停止浏览并关闭守护线程，忽略停止过程中的错误。
        let _ = daemon.stop_browse(AIMONITOR_SERVICE_TYPE);
        let _ = daemon.shutdown();
        // 按设备名排序返回，保证结果顺序稳定。
        let mut candidates: Vec<_> = candidates.into_values().collect();
        candidates.sort_by(|left, right| left.device.name.cmp(&right.device.name));
        Ok(candidates)
    }

    /// 对发现候选逐一做可达性探测，选出每个候选可用的地址；如果候选本身
    /// 探测失败，但恰好是当前已保存设备且保存地址可达，则回退使用保存地址。
    /// 若已保存设备完全不在候选列表中但仍可达，额外补一条"当前保存设备"记录，
    /// 避免用户当前使用的设备因为不在本轮发现结果里而从列表消失。
    pub(crate) async fn finish_device_discovery(
        &self,
        candidates: Vec<DiscoveryCandidate>,
    ) -> Result<Vec<DiscoveredMonitorDevice>, AppError> {
        let settings = self.settings()?;
        // 先探测一次当前已保存设备的地址是否可达，后面多处会用到这个结果。
        let saved_is_reachable =
            !settings.device_id.is_empty() && self.is_reachable(&settings.base_url).await;
        let mut devices = Vec::with_capacity(candidates.len() + 1);
        for mut candidate in candidates {
            if let Some(base_url) = self.first_reachable_url(&candidate.base_urls).await {
                // 候选自身有可达地址：直接采用第一个可达的 url。
                candidate.device.base_url = base_url;
            } else if saved_is_reachable && candidate.device.id == settings.device_id {
                // 候选本身探测失败，但恰好是当前保存设备且保存地址可达：回退用保存地址。
                candidate.device.base_url.clone_from(&settings.base_url);
                candidate.device.discovery_source = DiscoverySource::SavedAddress;
            } else {
                // 完全不可达则跳过该候选，不计入结果。
                continue;
            }
            devices.push(candidate.device);
        }

        // 判断当前保存设备是否已经出现在结果列表中（按 id 或 base_url 匹配）。
        let saved_is_known = devices
            .iter()
            .any(|device| device.id == settings.device_id || device.base_url == settings.base_url);
        // 若保存设备不在候选列表里但探测可达，额外补一条记录，避免它从列表消失。
        if !saved_is_known && saved_is_reachable {
            devices.push(DiscoveredMonitorDevice {
                id: settings.device_id,
                name: settings.device_name,
                api_version: "1".to_owned(),
                base_url: settings.base_url,
                path: DEFAULT_DEVICE_API_PATH.to_owned(),
                discovery_source: DiscoverySource::SavedAddress,
            });
        }

        // 按名称排序返回。
        devices.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(devices)
    }
}
