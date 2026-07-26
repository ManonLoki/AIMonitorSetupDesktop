use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{ErrorKind, Read, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock, mpsc},
    thread,
    time::{Duration, Instant},
};

use if_addrs::{IfAddr, IfOperStatus};
use mdns_sd::{ScopedIp, ServiceDaemon, ServiceEvent};
use reqwest::{Client, StatusCode, Url, header, multipart};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::domain::monitor::{
    AiProfile, AiTool, DEFAULT_HOOK_RELAY_PORT, DiscoveredMonitorDevice, DiscoverySource,
    HookBehavior, HookConfigDirectories, HookConfigLocation, HookConfigWriteResult, HookTransition,
    MonitorDeviceRoute, MonitorSettings, SavedMonitorData, ai_tool_name, encode_base64,
    generate_hook_config, hook_config_filename, hook_transition, is_authoritative_terminal_event,
    is_late_completion_event, merge_hook_config, normalize_base_url, validate_device_route,
    validate_discovery_interval_minutes, validate_profile, validate_saved_monitor_data,
    validate_username,
};

const STORE_FILENAME: &str = "monitor-data.json";
const AIMONITOR_SERVICE_TYPE: &str = "_aimonitor._tcp.local.";
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
const DISCOVERY_PROBE_TIMEOUT: Duration = Duration::from_millis(900);
const UDP_DISCOVERY_PORT: u16 = 8080;
const UDP_DISCOVERY_REQUEST: &[u8] = b"AIMONITOR_DISCOVER_V1";
const UDP_DISCOVERY_TIMEOUT: Duration = Duration::from_millis(1_200);
const UDP_RESPONSE_MAX_BYTES: usize = 1_024;
const DEFAULT_DEVICE_API_PATH: &str = "/api/device";
const MAX_REMOTE_IMAGE_BYTES: usize = 8 * 1024 * 1024;
pub const HOOK_LISTENER_PORT: u16 = DEFAULT_HOOK_RELAY_PORT;
const HOOK_BIND_ADDRESS: &str = "127.0.0.1";
const MAX_HOOK_REQUEST_BYTES: usize = 8 * 1024;
const MAX_HOOK_BODY_BYTES: usize = 2 * 1024;
const TERMINAL_STATE_GUARD: Duration = Duration::from_secs(2);
const DISCOVERY_MISSES_BEFORE_REMOVAL: u8 = 2;
/// 后台发现循环的轮询粒度：每次醒来都会重新读取当前配置的检查间隔，
/// 因此设置页修改间隔后，最多这么久就会生效，无需重启线程。
const DISCOVERY_POLL_GRANULARITY: Duration = Duration::from_secs(1);
pub const MONITOR_DEVICES_CHANGED_EVENT: &str = "monitor-devices-changed";

fn detect_hook_config_directories(config_home: &Path) -> HookConfigDirectories {
    HookConfigDirectories {
        codex: detected_config_directory("CODEX_HOME", &config_home.join(".codex")),
        claude_code: detected_config_directory("CLAUDE_CONFIG_DIR", &config_home.join(".claude")),
        cursor: config_home.join(".cursor").to_string_lossy().into_owned(),
    }
}

fn detected_config_directory(variable: &str, fallback: &Path) -> String {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| fallback.to_owned())
        .to_string_lossy()
        .into_owned()
}

/// 一次发现命中的设备，可能同时拥有多个候选地址（IPv4/IPv6、多网卡）；
/// 连接测试按 `candidate_url_priority` 排序后依次尝试，取第一个可达的。
#[derive(Clone, Debug)]
pub(crate) struct DiscoveryCandidate {
    device: DiscoveredMonitorDevice,
    base_urls: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct UdpBroadcastTarget {
    local_ip: Ipv4Addr,
    broadcast_ip: Ipv4Addr,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UdpDiscoveryResponse {
    id: String,
    name: String,
    port: u16,
    api_version: String,
}

fn discovery_base_url(address: &ScopedIp, port: u16) -> Option<String> {
    match address {
        ScopedIp::V4(address) => Some(format!("http://{}:{port}", address.addr())),
        ScopedIp::V6(address) if address.addr().is_unicast_link_local() => {
            let scope_id = address.scope_id().index;
            (scope_id != 0).then(|| format!("http://[{}%25{scope_id}]:{port}", address.addr()))
        }
        ScopedIp::V6(address) => Some(format!("http://[{}]:{port}", address.addr())),
        _ => None,
    }
}

/// IPv4 地址优先于 IPv6（`http://[` 形式，第 8 个字符是 `[`）：IPv6
/// 链路本地地址更容易受网卡切换、作用域 ID 失效等问题影响，连接稳定性更低。
fn candidate_url_priority(base_url: &str) -> u8 {
    u8::from(base_url.as_bytes().get(7) == Some(&b'['))
}

fn discover_udp_candidates() -> Result<Vec<DiscoveryCandidate>, String> {
    let targets = udp_broadcast_targets()?;
    discover_udp_on_targets(&targets, UDP_DISCOVERY_PORT, UDP_DISCOVERY_TIMEOUT)
}

fn udp_broadcast_targets() -> Result<Vec<UdpBroadcastTarget>, String> {
    let interfaces =
        if_addrs::get_if_addrs().map_err(|error| format!("无法枚举本机网卡：{error}"))?;
    let mut targets = interfaces
        .into_iter()
        .filter(|interface| {
            matches!(
                interface.oper_status,
                IfOperStatus::Up | IfOperStatus::Unknown
            ) && !interface.is_loopback()
                && !interface.is_p2p()
        })
        .filter_map(|interface| match interface.addr {
            IfAddr::V4(address) if !address.ip.is_unspecified() => {
                let broadcast_ip = address
                    .broadcast
                    .unwrap_or_else(|| directed_broadcast(address.ip, address.netmask));
                Some(UdpBroadcastTarget {
                    local_ip: address.ip,
                    broadcast_ip,
                })
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    targets.sort_by_key(|target| (target.local_ip, target.broadcast_ip));
    targets.dedup();

    if targets.is_empty() {
        targets.push(UdpBroadcastTarget {
            local_ip: Ipv4Addr::UNSPECIFIED,
            broadcast_ip: Ipv4Addr::BROADCAST,
        });
    }
    Ok(targets)
}

fn directed_broadcast(ip: Ipv4Addr, netmask: Ipv4Addr) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(ip) | !u32::from(netmask))
}

fn discover_udp_on_targets(
    targets: &[UdpBroadcastTarget],
    discovery_port: u16,
    timeout: Duration,
) -> Result<Vec<DiscoveryCandidate>, String> {
    let mut sockets = Vec::with_capacity(targets.len());
    let mut bind_errors = Vec::new();

    for target in targets {
        match UdpSocket::bind(SocketAddrV4::new(target.local_ip, 0)) {
            Ok(socket) => {
                if let Err(error) = socket.set_broadcast(true) {
                    bind_errors.push(format!("{}：{error}", target.local_ip));
                    continue;
                }
                if let Err(error) = socket.set_nonblocking(true) {
                    bind_errors.push(format!("{}：{error}", target.local_ip));
                    continue;
                }
                sockets.push((socket, *target));
            }
            Err(error) => bind_errors.push(format!("{}：{error}", target.local_ip)),
        }
    }

    if sockets.is_empty() {
        return Err(format!(
            "无法在任何 IPv4 网卡上创建 UDP socket：{}",
            bind_errors.join("；")
        ));
    }

    for _ in 0..2 {
        for (socket, target) in &sockets {
            let destinations = [target.broadcast_ip, Ipv4Addr::BROADCAST]
                .into_iter()
                .collect::<HashSet<_>>();
            for destination in destinations {
                let _ = socket.send_to(
                    UDP_DISCOVERY_REQUEST,
                    SocketAddrV4::new(destination, discovery_port),
                );
            }
        }
        thread::sleep(Duration::from_millis(75));
    }

    let deadline = Instant::now() + timeout;
    let mut candidates = HashMap::<String, DiscoveryCandidate>::new();
    let mut response = [0_u8; UDP_RESPONSE_MAX_BYTES];

    while Instant::now() < deadline {
        let mut received_any = false;
        for (socket, _) in &sockets {
            loop {
                match socket.recv_from(&mut response) {
                    Ok((length, source)) => {
                        received_any = true;
                        if let Some(device) =
                            parse_udp_discovery_response(&response[..length], source)
                        {
                            merge_udp_candidate(&mut candidates, device);
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }
        }
        if !received_any {
            thread::sleep(Duration::from_millis(10));
        }
    }

    let mut candidates = candidates.into_values().collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.device.name.cmp(&right.device.name));
    Ok(candidates)
}

fn parse_udp_discovery_response(
    bytes: &[u8],
    source: SocketAddr,
) -> Option<DiscoveredMonitorDevice> {
    let source_ip = match source {
        SocketAddr::V4(source) => *source.ip(),
        SocketAddr::V6(_) => return None,
    };
    if source_ip.is_unspecified() || source_ip.is_multicast() || source_ip.is_broadcast() {
        return None;
    }

    let response = serde_json::from_slice::<UdpDiscoveryResponse>(bytes).ok()?;
    let id = response.id.trim();
    let name = response.name.trim();
    let api_version = response.api_version.trim();
    if id.is_empty()
        || id.len() > 256
        || name.is_empty()
        || name.len() > 128
        || api_version.is_empty()
        || api_version.len() > 32
        || response.port == 0
    {
        return None;
    }

    Some(DiscoveredMonitorDevice {
        id: id.to_owned(),
        name: name.to_owned(),
        api_version: api_version.to_owned(),
        base_url: format!("http://{source_ip}:{}", response.port),
        path: DEFAULT_DEVICE_API_PATH.to_owned(),
        discovery_source: DiscoverySource::UdpBroadcast,
    })
}

fn merge_udp_candidate(
    candidates: &mut HashMap<String, DiscoveryCandidate>,
    device: DiscoveredMonitorDevice,
) {
    let base_url = device.base_url.clone();
    let candidate = candidates
        .entry(device.id.clone())
        .or_insert_with(|| DiscoveryCandidate {
            device,
            base_urls: Vec::new(),
        });
    if !candidate.base_urls.contains(&base_url) {
        candidate.base_urls.push(base_url);
    }
    candidate
        .base_urls
        .sort_by_key(|url| candidate_url_priority(url));
}

/// 按设备 id 合并 mDNS 和 UDP 广播两路发现结果：同一设备的候选地址取
/// 并集（mDNS 提供的地址通常更准确，优先排在前面），只在其中一路出现的
/// 设备原样保留，避免任何一台设备因为只被一种协议发现而从列表消失。
fn merge_discovery_candidates(
    mdns_candidates: Vec<DiscoveryCandidate>,
    udp_candidates: Vec<DiscoveryCandidate>,
) -> Vec<DiscoveryCandidate> {
    let mut merged = mdns_candidates
        .into_iter()
        .map(|candidate| (candidate.device.id.clone(), candidate))
        .collect::<HashMap<_, _>>();

    for udp_candidate in udp_candidates {
        merged
            .entry(udp_candidate.device.id.clone())
            .and_modify(|existing| {
                for base_url in &udp_candidate.base_urls {
                    if !existing.base_urls.contains(base_url) {
                        existing.base_urls.push(base_url.clone());
                    }
                }
                existing
                    .base_urls
                    .sort_by_key(|url| candidate_url_priority(url));
            })
            .or_insert(udp_candidate);
    }

    let mut merged = merged.into_values().collect::<Vec<_>>();
    merged.sort_by(|left, right| left.device.name.cmp(&right.device.name));
    merged
}

/// 对单轮偶发漏报做去抖：设备连续缺席两轮才从在线快照移除。mDNS/UDP 在
/// Wi-Fi 漫游或切换页面触发的并发扫描中可能丢一轮响应，直接替换列表会造成
/// “切换设备后少一台，稍后又恢复”的闪烁。
fn stabilize_discovered_devices(
    previous: &[DiscoveredMonitorDevice],
    mut discovered: Vec<DiscoveredMonitorDevice>,
    missed_scans: &mut HashMap<String, u8>,
) -> Vec<DiscoveredMonitorDevice> {
    let discovered_ids = discovered
        .iter()
        .map(|device| device.id.clone())
        .collect::<HashSet<_>>();
    missed_scans.retain(|id, _| !discovered_ids.contains(id));

    for device in previous {
        if discovered_ids.contains(&device.id) {
            continue;
        }
        let misses = missed_scans.entry(device.id.clone()).or_default();
        *misses = misses.saturating_add(1);
        if *misses < DISCOVERY_MISSES_BEFORE_REMOVAL {
            discovered.push(device.clone());
        }
    }
    discovered.sort_by(|left, right| left.name.cmp(&right.name));
    discovered
}

#[derive(Clone)]
pub struct MonitorService {
    client: Client,
    data_path: PathBuf,
    default_hook_config_directories: HookConfigDirectories,
    data: Arc<RwLock<SavedMonitorData>>,
    online_devices: Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
    discovery_missed_scans: Arc<Mutex<HashMap<String, u8>>>,
    hook_config_write_lock: Arc<Mutex<()>>,
    relay_status: Arc<RwLock<HookRelayStatus>>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRelayStatus {
    pub listening: bool,
    pub bind_address: String,
    pub port: u16,
    pub received_count: u64,
    pub forwarded_count: u64,
    pub failed_count: u64,
    pub suppressed_count: u64,
    pub pending_count: u64,
    pub last_tool: Option<AiTool>,
    pub last_hook_type: String,
    pub last_behavior: Option<HookBehavior>,
    pub last_error: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HookRequest {
    #[serde(rename = "type")]
    hook_type: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SlotUpdateRequest<'a> {
    username: &'a str,
    ai_name: &'static str,
    behavior: HookBehavior,
    content: &'a str,
    image: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub reachable: bool,
    pub base_url: String,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteImage {
    pub filename: String,
    pub mime_type: String,
    pub image: String,
}

#[derive(Deserialize)]
struct ImageListResponse {
    images: Vec<RemoteImageMetadata>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteImageMetadata {
    filename: String,
    mime_type: String,
}

#[derive(Deserialize)]
struct UploadResponse {
    filename: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageUpload {
    filename: String,
    mime_type: String,
    bytes: Vec<u8>,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
}

impl MonitorService {
    pub fn load(app_data_dir: &Path, config_home: &Path) -> Result<Self, String> {
        fs::create_dir_all(app_data_dir).map_err(|error| format!("无法创建配置目录：{error}"))?;
        let data_path = app_data_dir.join(STORE_FILENAME);
        let data = if data_path.exists() {
            let contents =
                fs::read_to_string(&data_path).map_err(|error| format!("无法读取配置：{error}"))?;
            serde_json::from_str(&contents).map_err(|error| format!("配置文件格式错误：{error}"))?
        } else {
            SavedMonitorData::default()
        };
        validate_saved_monitor_data(&data).map_err(|error| format!("配置数据校验失败：{error}"))?;
        Ok(Self {
            client: Client::new(),
            data_path,
            default_hook_config_directories: detect_hook_config_directories(config_home),
            data: Arc::new(RwLock::new(data)),
            online_devices: Arc::new(RwLock::new(Vec::new())),
            discovery_missed_scans: Arc::new(Mutex::new(HashMap::new())),
            hook_config_write_lock: Arc::new(Mutex::new(())),
            relay_status: Arc::new(RwLock::new(HookRelayStatus {
                bind_address: HOOK_BIND_ADDRESS.to_owned(),
                port: HOOK_LISTENER_PORT,
                ..HookRelayStatus::default()
            })),
        })
    }

    pub fn start_hook_listener(&self) {
        let data = Arc::clone(&self.data);
        let online_devices = Arc::clone(&self.online_devices);
        let status = Arc::clone(&self.relay_status);
        thread::spawn(move || {
            let listener = match TcpListener::bind((HOOK_BIND_ADDRESS, HOOK_LISTENER_PORT)) {
                Ok(listener) => listener,
                Err(error) => {
                    record_relay_failure(&status, format!("Hook 监听端口启动失败：{error}"));
                    return;
                }
            };
            if let Ok(mut current) = status.write() {
                current.listening = true;
                current.last_error.clear();
            }
            let client = reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(2))
                .timeout(Duration::from_secs(2))
                .build();
            let client = match client {
                Ok(client) => client,
                Err(error) => {
                    record_relay_failure(&status, format!("无法创建转发客户端：{error}"));
                    return;
                }
            };
            let (sender, receiver) = mpsc::channel::<(AiTool, String)>();
            let worker_data = Arc::clone(&data);
            let worker_status = Arc::clone(&status);
            thread::spawn(move || {
                let mut terminal_states = HashMap::<AiTool, Instant>::new();
                while let Ok((tool, hook_type)) = receiver.recv() {
                    if should_suppress_late_event(&mut terminal_states, tool, &hook_type) {
                        record_suppressed_hook(&worker_status, tool, &hook_type);
                        continue;
                    }
                    relay_hook(
                        &client,
                        &worker_data,
                        &online_devices,
                        &worker_status,
                        tool,
                        &hook_type,
                    );
                }
            });

            for connection in listener.incoming() {
                let Ok(mut stream) = connection else {
                    continue;
                };
                match read_hook_request(&mut stream) {
                    Ok((tool, hook_type)) => {
                        if let Ok(mut current) = status.write() {
                            current.pending_count += 1;
                        }
                        if sender.send((tool, hook_type)).is_ok() {
                            write_http_response(&mut stream, 202, "Accepted");
                        } else {
                            if let Ok(mut current) = status.write() {
                                current.pending_count = current.pending_count.saturating_sub(1);
                            }
                            write_http_response(&mut stream, 503, "Service Unavailable");
                            record_relay_failure(&status, "Hook 转发工作线程已停止".to_owned());
                        }
                    }
                    Err(error) => {
                        write_http_response(&mut stream, 400, "Bad Request");
                        record_relay_failure(&status, error);
                    }
                }
            }
            if let Ok(mut current) = status.write() {
                current.listening = false;
            }
        });
    }

    pub fn hook_relay_status(&self) -> Result<HookRelayStatus, String> {
        self.relay_status
            .read()
            .map(|status| status.clone())
            .map_err(|_| "Hook 服务状态读取锁已损坏".to_owned())
    }

    /// 在独立后台线程中立即发现一次设备，之后按设置页配置的间隔（默认一
    /// 分钟）持续刷新在线设备快照。循环以 1 秒粒度醒来并重新读取当前配置
    /// 的间隔，因此用户在设置页修改间隔后无需重启应用或线程即可立即生效。
    /// 只有快照实际变化时才向前端发送事件，避免无意义地重绘设备列表。
    pub fn start_background_device_discovery(&self, app: AppHandle) {
        let service = self.clone();
        thread::spawn(move || {
            let mut next_run = Instant::now();
            loop {
                if Instant::now() >= next_run {
                    let result = Self::discover_device_candidates().and_then(|candidates| {
                        tauri::async_runtime::block_on(service.finish_device_discovery(candidates))
                    });
                    if let Ok(devices) = result {
                        let _ = service.publish_online_devices(&app, devices);
                    }
                    next_run = Instant::now() + service.discovery_interval();
                }
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

    pub fn save_discovery_interval(&self, minutes: u64) -> Result<MonitorSettings, String> {
        let minutes = validate_discovery_interval_minutes(minutes)?;
        let mut data = self
            .data
            .write()
            .map_err(|_| "配置写入锁已损坏".to_owned())?;
        let mut next_data = data.clone();
        next_data.settings.discovery_interval_minutes = minutes;
        self.persist(&next_data)?;
        *data = next_data;
        Ok(data.settings.clone())
    }

    pub fn publish_online_devices(
        &self,
        app: &AppHandle,
        devices: Vec<DiscoveredMonitorDevice>,
    ) -> Result<Vec<DiscoveredMonitorDevice>, String> {
        let devices = if let Ok(mut missed_scans) = self.discovery_missed_scans.lock() {
            let previous = self
                .online_devices
                .read()
                .map_or_else(|_| Vec::new(), |current| current.clone());
            stabilize_discovered_devices(&previous, devices, &mut missed_scans)
        } else {
            devices
        };
        self.select_first_available_device_if_needed(&devices)?;
        if self.replace_online_devices(&devices) {
            let _ = app.emit(MONITOR_DEVICES_CHANGED_EVENT, devices.clone());
        }
        Ok(devices)
    }

    /// 保证当前选择始终指向在线快照中的设备。当前设备离线时按发现结果
    /// 的稳定顺序选择第一台在线设备，并通过 `select_device` 同步持久化路由。
    fn select_first_available_device_if_needed(
        &self,
        devices: &[DiscoveredMonitorDevice],
    ) -> Result<bool, String> {
        let Some(next) = devices.first() else {
            return Ok(false);
        };
        let settings = self.settings()?;
        if devices.iter().any(|device| device.id == settings.device_id) {
            return Ok(false);
        }
        self.select_device(next)?;
        Ok(true)
    }

    fn replace_online_devices(&self, devices: &[DiscoveredMonitorDevice]) -> bool {
        let Ok(mut current) = self.online_devices.write() else {
            return false;
        };
        if current.as_slice() == devices {
            return false;
        }
        *current = devices.to_vec();
        true
    }

    pub fn settings(&self) -> Result<MonitorSettings, String> {
        self.data
            .read()
            .map(|data| data.settings.clone())
            .map_err(|_| "配置读取锁已损坏".to_owned())
    }

    pub fn select_device(
        &self,
        device: &DiscoveredMonitorDevice,
    ) -> Result<MonitorSettings, String> {
        let route = validate_device_route(device)?;
        let mut data = self
            .data
            .write()
            .map_err(|_| "配置写入锁已损坏".to_owned())?;
        let mut next_data = data.clone();
        next_data.settings.base_url.clone_from(&route.base_url);
        next_data.settings.device_id.clone_from(&route.device_id);
        next_data
            .settings
            .device_name
            .clone_from(&route.device_name);
        next_data
            .devices
            .retain(|existing| existing.device_id != route.device_id);
        next_data.devices.push(route);
        next_data
            .devices
            .sort_by(|left, right| left.device_id.cmp(&right.device_id));
        self.persist(&next_data)?;
        *data = next_data;
        Ok(data.settings.clone())
    }

    pub fn save_username(&self, username: &str) -> Result<MonitorSettings, String> {
        let username = validate_username(username)?;
        let mut data = self
            .data
            .write()
            .map_err(|_| "配置写入锁已损坏".to_owned())?;
        let mut next_data = data.clone();
        next_data.settings.username = username;
        self.persist(&next_data)?;
        *data = next_data;
        Ok(data.settings.clone())
    }

    /// 设备发现的主入口：mDNS 和 UDP 广播总是都跑一遍并按设备 id 合并结果，
    /// 而不是任一路先返回非空结果就跳过另一路——两台设备可能只有其中一台
    /// 的 mDNS 广播能穿透当前网络（多播被 AP 丢弃、跨 VLAN 等），仅在其中一路
    /// 发现了设备时就放弃另一路会把只靠 UDP 广播现身的设备从列表里漏掉。
    /// 只有两路都出错才报错。
    pub(crate) fn discover_device_candidates() -> Result<Vec<DiscoveryCandidate>, String> {
        let (mdns_result, udp_result) = thread::scope(|scope| {
            let mdns = scope.spawn(Self::discover_mdns_candidates);
            let udp = scope.spawn(discover_udp_candidates);
            (
                mdns.join()
                    .unwrap_or_else(|_| Err("mDNS 发现线程异常退出".to_owned())),
                udp.join()
                    .unwrap_or_else(|_| Err("UDP 发现线程异常退出".to_owned())),
            )
        });
        match (mdns_result, udp_result) {
            (Ok(mdns_candidates), Ok(udp_candidates)) => {
                Ok(merge_discovery_candidates(mdns_candidates, udp_candidates))
            }
            (Ok(candidates), Err(_)) | (Err(_), Ok(candidates)) => Ok(candidates),
            (Err(mdns_error), Err(udp_error)) => Err(format!(
                "mDNS 发现失败：{mdns_error}；UDP 广播发现失败：{udp_error}"
            )),
        }
    }

    fn discover_mdns_candidates() -> Result<Vec<DiscoveryCandidate>, String> {
        let daemon = ServiceDaemon::new().map_err(|error| format!("无法启动设备发现：{error}"))?;
        daemon
            .set_ip_check_interval(1)
            .map_err(|error| format!("无法启用网卡刷新：{error}"))?;
        let receiver = daemon
            .browse(AIMONITOR_SERVICE_TYPE)
            .map_err(|error| format!("无法扫描 AIMonitor 设备：{error}"))?;
        let deadline = Instant::now() + DISCOVERY_TIMEOUT;
        let mut candidates: HashMap<String, DiscoveryCandidate> = HashMap::new();

        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            match receiver.recv_timeout(remaining) {
                Ok(ServiceEvent::ServiceResolved(service)) => {
                    let properties = service.get_properties();
                    let id = properties
                        .get_property_val_str("id")
                        .unwrap_or(service.get_fullname())
                        .to_owned();
                    let name = properties
                        .get_property_val_str("name")
                        .unwrap_or(service.get_fullname())
                        .to_owned();
                    let api_version = properties
                        .get_property_val_str("apiVersion")
                        .unwrap_or("1")
                        .to_owned();
                    let path = properties
                        .get_property_val_str("path")
                        .unwrap_or(DEFAULT_DEVICE_API_PATH)
                        .to_owned();
                    let mut base_urls = service
                        .get_addresses()
                        .iter()
                        .filter_map(|address| discovery_base_url(address, service.get_port()))
                        .collect::<Vec<_>>();
                    if base_urls.is_empty() {
                        let host = service.get_hostname().trim_end_matches('.');
                        if !host.is_empty() {
                            base_urls.push(format!("http://{host}:{}", service.get_port()));
                        }
                    }
                    base_urls.sort_by_key(|url| candidate_url_priority(url));
                    base_urls.dedup();

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
                    candidate.device.name = name;
                    candidate.device.api_version = api_version;
                    candidate.device.path = path;
                    candidate.base_urls.extend(base_urls);
                    candidate
                        .base_urls
                        .sort_by_key(|url| candidate_url_priority(url));
                    candidate.base_urls.dedup();
                    if let Some(base_url) = candidate.base_urls.first() {
                        candidate.device.base_url.clone_from(base_url);
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }

        let _ = daemon.stop_browse(AIMONITOR_SERVICE_TYPE);
        let _ = daemon.shutdown();
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
    ) -> Result<Vec<DiscoveredMonitorDevice>, String> {
        let settings = self.settings()?;
        let saved_is_reachable =
            !settings.device_id.is_empty() && self.is_reachable(&settings.base_url).await;
        let mut devices = Vec::with_capacity(candidates.len() + 1);
        for mut candidate in candidates {
            if let Some(base_url) = self.first_reachable_url(&candidate.base_urls).await {
                candidate.device.base_url = base_url;
            } else if saved_is_reachable && candidate.device.id == settings.device_id {
                candidate.device.base_url.clone_from(&settings.base_url);
                candidate.device.discovery_source = DiscoverySource::SavedAddress;
            } else {
                continue;
            }
            devices.push(candidate.device);
        }

        let saved_is_known = devices
            .iter()
            .any(|device| device.id == settings.device_id || device.base_url == settings.base_url);
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

        devices.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(devices)
    }

    pub async fn check_connection(
        &self,
        base_url: Option<&str>,
    ) -> Result<ConnectionStatus, String> {
        let base_url = match base_url {
            Some(value) => normalize_base_url(value)?,
            None => self.settings()?.base_url,
        };
        let result = self
            .client
            .get(format!("{base_url}/health"))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;

        Ok(match result {
            Ok(response) if response.status().is_success() => ConnectionStatus {
                reachable: true,
                base_url,
                message: "设备连接正常".to_owned(),
            },
            Ok(response) => ConnectionStatus {
                reachable: false,
                base_url,
                message: format!("设备返回 HTTP {}", response.status().as_u16()),
            },
            Err(error) => ConnectionStatus {
                reachable: false,
                base_url,
                message: format!("无法连接设备：{error}"),
            },
        })
    }

    async fn first_reachable_url(&self, base_urls: &[String]) -> Option<String> {
        for base_url in base_urls {
            if self.is_reachable(base_url).await {
                return Some(base_url.clone());
            }
        }
        None
    }

    async fn is_reachable(&self, base_url: &str) -> bool {
        matches!(
            self.client
                .get(format!("{base_url}/health"))
                .timeout(DISCOVERY_PROBE_TIMEOUT)
                .send()
                .await,
            Ok(response) if response.status().is_success()
        )
    }

    pub async fn images(&self) -> Result<Vec<RemoteImage>, String> {
        let base_url = self.settings()?.base_url;
        let response = self
            .client
            .get(format!("{base_url}/api/images"))
            .send()
            .await
            .map_err(|error| format!("无法读取远端图片：{error}"))?;
        let response = ensure_success(response).await?;
        let metadata = response
            .json::<ImageListResponse>()
            .await
            .map(|body| body.images)
            .map_err(|error| format!("图片列表响应格式错误：{error}"))?;
        let mut images = Vec::with_capacity(metadata.len());

        // AIMonitor serves image bytes through GET /api/images/{filename}.
        // Fetch sequentially because the embedded device handles only a small
        // number of concurrent connections reliably.
        for item in metadata {
            images.push(self.remote_image(&base_url, item).await?);
        }

        Ok(images)
    }

    async fn remote_image(
        &self,
        base_url: &str,
        metadata: RemoteImageMetadata,
    ) -> Result<RemoteImage, String> {
        let filename = metadata.filename.trim();
        let url = remote_image_url(base_url, filename)?;
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| format!("{filename} 读取失败：{error}"))?;
        let response = ensure_success(response).await?;
        if let Some(length) = response.content_length() {
            ensure_image_size(length, filename)?;
        }

        let header_mime = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::trim);
        let mime_type = [header_mime, Some(metadata.mime_type.trim())]
            .into_iter()
            .flatten()
            .find(|value| is_supported_image_mime(value))
            .ok_or_else(|| format!("{filename} 返回了不支持的图片类型"))?
            .to_owned();

        let bytes = response
            .bytes()
            .await
            .map_err(|error| format!("{filename} 读取失败：{error}"))?;
        ensure_image_size(bytes.len() as u64, filename)?;

        let image = format!("data:{mime_type};base64,{}", encode_base64(&bytes));
        Ok(RemoteImage {
            filename: filename.to_owned(),
            mime_type,
            image,
        })
    }

    pub async fn upload_images(&self, images: Vec<ImageUpload>) -> Result<Vec<String>, String> {
        validate_image_uploads(&images)?;
        let base_url = self.settings()?.base_url;
        let mut uploaded = Vec::with_capacity(images.len());

        for image in images {
            let filename = image.filename.clone();
            let file_part = multipart::Part::bytes(image.bytes)
                .file_name(filename.clone())
                .mime_str(&image.mime_type)
                .map_err(|error| format!("{filename} 的图片类型无效：{error}"))?;
            let response = self
                .client
                .post(format!("{base_url}/api/images"))
                .multipart(multipart::Form::new().part("file", file_part))
                .send()
                .await
                .map_err(|error| format!("{filename} 上传失败：{error}"))?;
            let response = ensure_success(response).await?;
            let uploaded_filename = response
                .json::<UploadResponse>()
                .await
                .map(|body| body.filename)
                .map_err(|error| format!("{filename} 的上传响应格式错误：{error}"))?;
            uploaded.push(uploaded_filename);
        }

        Ok(uploaded)
    }

    pub async fn delete_image(&self, filename: &str) -> Result<(), String> {
        let base_url = self.settings()?.base_url;
        let response = self
            .client
            .delete(format!("{base_url}/api/images/{filename}"))
            .send()
            .await
            .map_err(|error| format!("删除图片失败：{error}"))?;
        ensure_success(response).await?;
        Ok(())
    }

    pub fn profiles(&self) -> Result<Vec<AiProfile>, String> {
        self.data
            .read()
            .map(|data| {
                data.profiles
                    .iter()
                    .filter(|profile| profile.device_id == data.settings.device_id)
                    .cloned()
                    .collect()
            })
            .map_err(|_| "AI 配置读取锁已损坏".to_owned())
    }

    pub fn hook_config_locations(&self) -> Result<Vec<HookConfigLocation>, String> {
        let data = self
            .data
            .read()
            .map_err(|_| "Hooks 路径读取锁已损坏".to_owned())?;
        Ok(AiTool::ALL
            .into_iter()
            .map(|tool| self.hook_config_location(&data, tool))
            .collect())
    }

    pub fn save_hook_config_directory(
        &self,
        tool: AiTool,
        directory: &str,
    ) -> Result<HookConfigLocation, String> {
        let directory = directory.trim();
        if !directory.is_empty() {
            let path = Path::new(directory);
            if !path.is_absolute() {
                return Err("Hooks 配置目录必须使用绝对路径".to_owned());
            }
            if path.exists() && !path.is_dir() {
                return Err(format!("Hooks 配置目录不是文件夹：{}", path.display()));
            }
        }

        let mut data = self
            .data
            .write()
            .map_err(|_| "Hooks 路径写入锁已损坏".to_owned())?;
        let mut next_data = data.clone();
        next_data
            .hook_config_directories
            .set(tool, directory.to_owned());
        self.persist(&next_data)?;
        let location = self.hook_config_location(&next_data, tool);
        *data = next_data;
        Ok(location)
    }

    pub fn save_profile(&self, profile: AiProfile) -> Result<AiProfile, String> {
        let mut data = self
            .data
            .write()
            .map_err(|_| "AI 配置写入锁已损坏".to_owned())?;
        if data.settings.device_id.is_empty() {
            return Err("请先选择 AIMonitor 设备".to_owned());
        }
        let mut profile = profile;
        profile.device_id.clone_from(&data.settings.device_id);
        let profile = validate_profile(profile)?;
        let mut next_data = data.clone();
        next_data.profiles.retain(|existing| {
            existing.device_id != profile.device_id || existing.tool != profile.tool
        });
        next_data.profiles.push(profile.clone());
        next_data.profiles.sort_by(|left, right| {
            left.device_id
                .cmp(&right.device_id)
                .then(left.slot.cmp(&right.slot))
        });
        self.persist(&next_data)?;
        *data = next_data;
        Ok(profile)
    }

    pub fn write_hook_config(&self, tool: AiTool) -> Result<HookConfigWriteResult, String> {
        let _write_guard = self
            .hook_config_write_lock
            .lock()
            .map_err(|_| "Hooks 配置写入锁已损坏".to_owned())?;
        let data = self
            .data
            .read()
            .map_err(|_| "Hooks 配置读取锁已损坏".to_owned())?;
        let profile = data
            .profiles
            .iter()
            .find(|profile| profile.device_id == data.settings.device_id && profile.tool == tool)
            .cloned()
            .ok_or_else(|| "请先在监控管理中保存该工具的展示配置".to_owned())?;
        let generated = generate_hook_config(profile)?;
        let config_path = self.hook_config_path(&data, tool);
        let existing = read_optional_config(&config_path)?;
        let mut merged = merge_hook_config(existing.as_deref(), &generated, tool)?;
        merged.filename = config_path.to_string_lossy().into_owned();
        let config_changed = existing.as_deref() != Some(merged.content.as_str());
        if config_changed {
            write_config(&config_path, &merged.content)?;
        }
        Ok(HookConfigWriteResult {
            requires_review: tool == AiTool::Codex && config_changed,
            restart_required: tool == AiTool::Codex && config_changed,
            tool,
            filename: merged.filename,
            config_changed,
        })
    }

    fn hook_config_location(&self, data: &SavedMonitorData, tool: AiTool) -> HookConfigLocation {
        let custom_directory = data.hook_config_directories.get(tool);
        let directory = if custom_directory.is_empty() {
            self.default_hook_config_directories.get(tool)
        } else {
            custom_directory
        };
        let config_path = Path::new(directory).join(hook_config_filename(tool));
        HookConfigLocation {
            tool,
            directory: directory.to_owned(),
            config_path: config_path.to_string_lossy().into_owned(),
            is_custom: !custom_directory.is_empty(),
        }
    }

    fn hook_config_path(&self, data: &SavedMonitorData, tool: AiTool) -> PathBuf {
        PathBuf::from(self.hook_config_location(data, tool).config_path)
    }

    fn persist(&self, data: &SavedMonitorData) -> Result<(), String> {
        let serialized = serde_json::to_string_pretty(data)
            .map_err(|error| format!("无法序列化配置：{error}"))?;
        write_atomic_file(&self.data_path, &serialized, "应用配置")
    }
}

fn read_hook_request(stream: &mut TcpStream) -> Result<(AiTool, String), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| format!("无法设置 Hook 请求超时：{error}"))?;
    let mut request = Vec::with_capacity(1024);
    let mut buffer = [0_u8; 1024];
    let header_end = loop {
        let read = stream
            .read(&mut buffer)
            .map_err(|error| format!("无法读取 Hook 请求：{error}"))?;
        if read == 0 {
            return Err("Hook 请求未完整发送".to_owned());
        }
        request.extend_from_slice(&buffer[..read]);
        if request.len() > MAX_HOOK_REQUEST_BYTES {
            return Err("Hook 请求过大".to_owned());
        }
        if let Some(index) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break index + 4;
        }
    };

    let headers = std::str::from_utf8(&request[..header_end])
        .map_err(|_| "Hook 请求头不是有效 UTF-8".to_owned())?;
    let mut lines = headers.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| "Hook 请求缺少请求行".to_owned())?;
    let mut request_parts = request_line.split_whitespace();
    if request_parts.next() != Some("POST") {
        return Err("Hook 接口只接受 POST".to_owned());
    }
    let path = request_parts
        .next()
        .ok_or_else(|| "Hook 请求缺少路径".to_owned())?;
    let tool = match path.strip_prefix("/api/hooks/") {
        Some("codex") => AiTool::Codex,
        Some("claude-code") => AiTool::ClaudeCode,
        Some("cursor") => AiTool::Cursor,
        _ => return Err("Hook 请求中的 AI 工具无效".to_owned()),
    };
    let content_length = lines
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .ok_or_else(|| "Hook 请求缺少有效的 Content-Length".to_owned())?;
    if content_length == 0 || content_length > MAX_HOOK_BODY_BYTES {
        return Err("Hook 请求体大小无效".to_owned());
    }

    while request.len() < header_end + content_length {
        let read = stream
            .read(&mut buffer)
            .map_err(|error| format!("无法读取 Hook 请求体：{error}"))?;
        if read == 0 {
            return Err("Hook 请求体未完整发送".to_owned());
        }
        request.extend_from_slice(&buffer[..read]);
        if request.len() > MAX_HOOK_REQUEST_BYTES {
            return Err("Hook 请求过大".to_owned());
        }
    }
    let body =
        serde_json::from_slice::<HookRequest>(&request[header_end..header_end + content_length])
            .map_err(|error| format!("Hook 请求 JSON 无效：{error}"))?;
    let hook_type = body.hook_type.trim();
    if hook_type.is_empty() || hook_type.len() > 128 {
        return Err("Hook 类型不能为空且不能超过 128 个字符".to_owned());
    }
    Ok((tool, hook_type.to_owned()))
}

fn relay_hook(
    client: &reqwest::blocking::Client,
    data: &Arc<RwLock<SavedMonitorData>>,
    online_devices: &Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
    status: &Arc<RwLock<HookRelayStatus>>,
    tool: AiTool,
    hook_type: &str,
) {
    let Some(transition) = hook_transition(tool, hook_type) else {
        record_hook_results(
            status,
            tool,
            hook_type,
            None,
            0,
            &[format!("不支持的 Hook 类型：{hook_type}")],
        );
        return;
    };
    let Ok(data) = data.read() else {
        record_hook_results(
            status,
            tool,
            hook_type,
            None,
            0,
            &["转发配置读取锁已损坏".to_owned()],
        );
        return;
    };
    let snapshot = data.clone();
    drop(data);
    let online_snapshot = online_devices
        .read()
        .map(|devices| devices.clone())
        .unwrap_or_default();
    let online_ids = online_snapshot
        .iter()
        .map(|device| device.id.as_str())
        .collect::<HashSet<_>>();
    let mut targets = snapshot
        .profiles
        .iter()
        .filter(|profile| profile.tool == tool)
        .filter_map(|profile| {
            snapshot
                .devices
                .iter()
                .find(|device| device.device_id == profile.device_id)
                .map(|device| (device, profile))
        })
        .collect::<Vec<_>>();
    targets.sort_by_key(|(device, _)| !online_ids.contains(device.device_id.as_str()));
    if targets.is_empty() {
        record_hook_results(
            status,
            tool,
            hook_type,
            None,
            0,
            &["尚未配置该 AI 的转发位置".to_owned()],
        );
        return;
    }

    let behavior = match transition {
        HookTransition::Display(behavior) => Some(behavior),
        HookTransition::Release => None,
    };
    let (forwarded, errors) = forward_to_all_targets(
        client,
        tool,
        transition,
        &snapshot.settings.username,
        targets,
        &online_snapshot,
    );
    record_hook_results(status, tool, hook_type, behavior, forwarded, &errors);
}

/// 并发转发给每台已配置该 AI 的设备：先一次性 spawn 所有目标的转发线程，
/// 再统一 join，这样单台设备网络慢或不可达时不会拖慢其余设备收到状态更新的
/// 时间；每台设备的成功/失败互不影响。
fn forward_to_all_targets(
    client: &reqwest::blocking::Client,
    tool: AiTool,
    transition: HookTransition,
    username: &str,
    targets: Vec<(&MonitorDeviceRoute, &AiProfile)>,
    online_snapshot: &[DiscoveredMonitorDevice],
) -> (u64, Vec<String>) {
    let outcomes = thread::scope(|scope| {
        targets
            .into_iter()
            .map(|(saved_device, profile)| {
                let online_device = online_snapshot
                    .iter()
                    .find(|device| device.id == saved_device.device_id);
                let effective_device = online_device.map_or_else(
                    || saved_device.clone(),
                    |device| MonitorDeviceRoute {
                        base_url: device.base_url.clone(),
                        device_id: device.id.clone(),
                        device_name: device.name.clone(),
                    },
                );
                scope.spawn(move || {
                    let result = forward_profile(
                        client,
                        tool,
                        transition,
                        username,
                        &effective_device,
                        profile,
                    );
                    (effective_device.device_name, result)
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| ("未知设备".to_owned(), Err("转发线程异常终止".to_owned())))
            })
            .collect::<Vec<_>>()
    });

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

fn forward_profile(
    client: &reqwest::blocking::Client,
    tool: AiTool,
    transition: HookTransition,
    username: &str,
    device: &MonitorDeviceRoute,
    profile: &AiProfile,
) -> Result<(), String> {
    if device.base_url.is_empty() || username.is_empty() {
        return Err("设备地址或显示用户名为空".to_owned());
    }
    let url = format!("{}/api/slots/{}", device.base_url, profile.slot);
    match transition {
        HookTransition::Display(behavior) => profile
            .hooks
            .iter()
            .find(|state| state.behavior == behavior)
            .ok_or_else(|| "AI 状态配置不完整".to_owned())
            .and_then(|state| {
                client
                    .post(&url)
                    .json(&SlotUpdateRequest {
                        username,
                        ai_name: ai_tool_name(tool),
                        behavior,
                        content: &state.content,
                        image: &state.image,
                    })
                    .send()
                    .map_err(|error| format!("转发到监控屏失败：{error}"))
                    .and_then(|response| {
                        ensure_success_blocking(response)
                            .map(|_| ())
                            .map_err(|error| format!("监控屏拒绝了状态更新：{error}"))
                    })
            }),
        HookTransition::Release => client
            .delete(&url)
            .send()
            .map_err(|error| format!("释放监控屏位置失败：{error}"))
            .and_then(|response| {
                ensure_success_blocking(response)
                    .map(|_| ())
                    .map_err(|error| format!("监控屏拒绝了位置释放：{error}"))
            }),
    }
}

fn ensure_success_blocking(
    response: reqwest::blocking::Response,
) -> Result<reqwest::blocking::Response, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let fallback = format!("设备请求失败（HTTP {}）", status.as_u16());
    if status == StatusCode::NO_CONTENT {
        return Err(fallback);
    }
    let message = response
        .json::<ErrorResponse>()
        .map_or(fallback, |body| body.error);
    Err(message)
}

fn record_hook_results(
    status: &Arc<RwLock<HookRelayStatus>>,
    tool: AiTool,
    hook_type: &str,
    behavior: Option<HookBehavior>,
    forwarded: u64,
    errors: &[String],
) {
    if let Ok(mut current) = status.write() {
        current.received_count += 1;
        current.pending_count = current.pending_count.saturating_sub(1);
        current.forwarded_count += forwarded;
        current.failed_count += errors.len() as u64;
        current.last_tool = Some(tool);
        hook_type.clone_into(&mut current.last_hook_type);
        current.last_behavior = behavior;
        current.last_error = errors.join("；");
    }
}

fn should_suppress_late_event(
    terminal_states: &mut HashMap<AiTool, Instant>,
    tool: AiTool,
    hook_type: &str,
) -> bool {
    let now = Instant::now();
    if is_authoritative_terminal_event(tool, hook_type) {
        terminal_states.insert(tool, now);
        return false;
    }
    if is_late_completion_event(hook_type)
        && terminal_states
            .get(&tool)
            .is_some_and(|terminal| now.duration_since(*terminal) <= TERMINAL_STATE_GUARD)
    {
        return true;
    }
    if !is_late_completion_event(hook_type) {
        terminal_states.remove(&tool);
    }
    false
}

fn record_suppressed_hook(status: &Arc<RwLock<HookRelayStatus>>, tool: AiTool, hook_type: &str) {
    if let Ok(mut current) = status.write() {
        current.received_count += 1;
        current.suppressed_count += 1;
        current.pending_count = current.pending_count.saturating_sub(1);
        current.last_tool = Some(tool);
        hook_type.clone_into(&mut current.last_hook_type);
        current.last_behavior = Some(HookBehavior::Idle);
        current.last_error.clear();
    }
}

fn record_relay_failure(status: &Arc<RwLock<HookRelayStatus>>, error: String) {
    if let Ok(mut current) = status.write() {
        current.failed_count += 1;
        current.last_error = error;
    }
}

fn write_http_response(stream: &mut TcpStream, status: u16, reason: &str) {
    let response =
        format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn remote_image_url(base_url: &str, filename: &str) -> Result<Url, String> {
    if filename.is_empty() || filename == "." || filename == ".." || filename.contains(['/', '\\'])
    {
        return Err("远端图片文件名无效".to_owned());
    }

    let mut url = Url::parse(&format!("{base_url}/api/images/"))
        .map_err(|error| format!("设备图片地址无效：{error}"))?;
    url.path_segments_mut()
        .map_err(|()| "设备图片地址不能包含路径段".to_owned())?
        .pop_if_empty()
        .push(filename);
    Ok(url)
}

fn is_supported_image_mime(mime_type: &str) -> bool {
    matches!(mime_type, "image/jpeg" | "image/png" | "image/gif")
}

fn ensure_image_size(len: u64, filename: &str) -> Result<(), String> {
    if len > MAX_REMOTE_IMAGE_BYTES as u64 {
        return Err(format!("{filename} 不能超过 8 MiB"));
    }
    Ok(())
}

fn validate_image_uploads(images: &[ImageUpload]) -> Result<(), String> {
    if images.is_empty() {
        return Err("请选择要上传的图片".to_owned());
    }

    for image in images {
        if image.filename.trim().is_empty() || image.bytes.is_empty() {
            return Err("所选图片中包含空文件".to_owned());
        }
        ensure_image_size(image.bytes.len() as u64, &image.filename)?;
        if !is_supported_image_mime(&image.mime_type) {
            return Err(format!(
                "{} 不是支持的 JPEG、PNG 或 GIF 图片",
                image.filename
            ));
        }
    }

    Ok(())
}

fn read_optional_config(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("无法读取 {}：{error}", path.display())),
    }
}

fn write_config(path: &Path, content: &str) -> Result<(), String> {
    write_atomic_file(path, content, "Hooks 配置")
}

fn write_atomic_file(path: &Path, content: &str, label: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("无法确定 {} 的配置目录", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("无法创建配置目录 {}：{error}", parent.display()))?;

    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("配置文件路径无效：{}", path.display()))?;
    let temporary_path = parent.join(format!(".{filename}.aimonitor.tmp"));
    fs::write(&temporary_path, content)
        .map_err(|error| format!("无法写入临时配置 {}：{error}", temporary_path.display()))?;

    #[cfg(not(windows))]
    let replace_result = fs::rename(&temporary_path, path);
    #[cfg(windows)]
    let replace_result = fs::write(path, content).and_then(|()| fs::remove_file(&temporary_path));

    if let Err(error) = replace_result {
        let _ = fs::remove_file(&temporary_path);
        return Err(format!("无法写入{label} {}：{error}", path.display()));
    }
    Ok(())
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let fallback = format!("设备请求失败（HTTP {}）", status.as_u16());
    if status == StatusCode::NO_CONTENT {
        return Err(fallback);
    }
    let message = response
        .json::<ErrorResponse>()
        .await
        .map_or(fallback, |body| body.error);
    Err(message)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::time::{SystemTime, UNIX_EPOCH};

    use mdns_sd::{InterfaceId, ScopedIpV4};

    use crate::domain::monitor::{HookBehavior, HookContent};

    use super::*;

    fn test_profile() -> AiProfile {
        AiProfile {
            device_id: "screen-1".to_owned(),
            tool: AiTool::Codex,
            slot: 1,
            hooks: [
                (HookBehavior::Idle, "idle.png"),
                (HookBehavior::Running, "running.gif"),
                (HookBehavior::Asking, "asking.png"),
                (HookBehavior::Error, "error.png"),
            ]
            .into_iter()
            .map(|(behavior, image)| HookContent {
                behavior,
                content: String::new(),
                image: image.to_owned(),
            })
            .collect(),
        }
    }

    #[test]
    fn local_hook_request_accepts_only_tool_path_and_type_body() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let body = r#"{"type":"SessionStart"}"#;
        let request = format!(
            "POST /api/hooks/codex HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let sender = thread::spawn(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            stream.write_all(request.as_bytes()).unwrap();
        });
        let (mut stream, _) = listener.accept().unwrap();

        let request = read_hook_request(&mut stream).unwrap();

        sender.join().unwrap();
        assert_eq!(request, (AiTool::Codex, "SessionStart".to_owned()));
    }

    #[test]
    fn local_hook_request_rejects_business_payload_fields() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let body = r#"{"type":"Stop","image":"should-not-be-here.png"}"#;
        let request = format!(
            "POST /api/hooks/cursor HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let sender = thread::spawn(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            stream.write_all(request.as_bytes()).unwrap();
        });
        let (mut stream, _) = listener.accept().unwrap();

        let error = read_hook_request(&mut stream).unwrap_err();

        sender.join().unwrap();
        assert!(error.contains("unknown field"));
    }

    #[test]
    fn relay_computes_state_and_uses_a_configured_device_route() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let receiver = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 2048];
            loop {
                let length = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..length]);
                let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                else {
                    continue;
                };
                let header_end = header_end + 4;
                let headers = std::str::from_utf8(&request[..header_end]).unwrap();
                let content_length = headers
                    .split("\r\n")
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                if request.len() >= header_end + content_length {
                    break;
                }
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
            String::from_utf8(request).unwrap()
        });
        let data = Arc::new(RwLock::new(SavedMonitorData {
            settings: MonitorSettings {
                base_url: format!("http://{address}"),
                username: "Manon".to_owned(),
                device_id: "screen-1".to_owned(),
                device_name: "Desk".to_owned(),
                ..MonitorSettings::default()
            },
            devices: vec![MonitorDeviceRoute {
                base_url: format!("http://{address}"),
                device_id: "screen-1".to_owned(),
                device_name: "Desk".to_owned(),
            }],
            profiles: vec![test_profile()],
            hook_config_directories: HookConfigDirectories::default(),
        }));
        let status = Arc::new(RwLock::new(HookRelayStatus::default()));
        let online_devices = Arc::new(RwLock::new(Vec::new()));

        relay_hook(
            &reqwest::blocking::Client::new(),
            &data,
            &online_devices,
            &status,
            AiTool::Codex,
            "UserPromptSubmit",
        );

        let request = receiver.join().unwrap();
        assert!(request.starts_with("POST /api/slots/1 HTTP/1.1"));
        assert!(request.contains(r#""username":"Manon""#));
        assert!(request.contains(r#""aiName":"Codex""#));
        assert!(request.contains(r#""behavior":"running""#));
        assert!(request.contains(r#""image":"running.gif""#));
        let status = status.read().unwrap();
        assert_eq!(status.received_count, 1);
        assert_eq!(status.forwarded_count, 1);
        assert_eq!(status.last_behavior, Some(HookBehavior::Running));
        assert!(status.last_error.is_empty());
    }

    #[test]
    fn relay_prioritizes_online_routes_and_uses_their_latest_address() {
        let unavailable_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let unavailable_address = unavailable_listener.local_addr().unwrap();
        drop(unavailable_listener);

        let available_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let available_address = available_listener.local_addr().unwrap();
        let receiver = thread::spawn(move || {
            let (mut stream, _) = available_listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 2048];
            loop {
                let length = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..length]);
                let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                else {
                    continue;
                };
                let header_end = header_end + 4;
                let headers = std::str::from_utf8(&request[..header_end]).unwrap();
                let content_length = headers
                    .split("\r\n")
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                if request.len() >= header_end + content_length {
                    break;
                }
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
            String::from_utf8(request).unwrap()
        });

        let mut available_profile = test_profile();
        available_profile.device_id = "screen-2".to_owned();
        available_profile.slot = 7;
        let data = Arc::new(RwLock::new(SavedMonitorData {
            settings: MonitorSettings {
                base_url: format!("http://{unavailable_address}"),
                username: "Desk user".to_owned(),
                device_id: "screen-1".to_owned(),
                device_name: "Desk".to_owned(),
                ..MonitorSettings::default()
            },
            devices: vec![
                MonitorDeviceRoute {
                    base_url: format!("http://{unavailable_address}"),
                    device_id: "screen-1".to_owned(),
                    device_name: "Desk".to_owned(),
                },
                MonitorDeviceRoute {
                    base_url: format!("http://{unavailable_address}"),
                    device_id: "screen-2".to_owned(),
                    device_name: "Studio".to_owned(),
                },
            ],
            profiles: vec![test_profile(), available_profile],
            hook_config_directories: HookConfigDirectories::default(),
        }));
        let status = Arc::new(RwLock::new(HookRelayStatus::default()));
        let online_devices = Arc::new(RwLock::new(vec![DiscoveredMonitorDevice {
            id: "screen-2".to_owned(),
            name: "Studio".to_owned(),
            api_version: "1".to_owned(),
            base_url: format!("http://{available_address}"),
            path: DEFAULT_DEVICE_API_PATH.to_owned(),
            discovery_source: DiscoverySource::Mdns,
        }]));

        relay_hook(
            &reqwest::blocking::Client::new(),
            &data,
            &online_devices,
            &status,
            AiTool::Codex,
            "UserPromptSubmit",
        );

        let request = receiver.join().unwrap();
        assert!(request.starts_with("POST /api/slots/7 HTTP/1.1"));
        assert!(request.contains(r#""username":"Desk user""#));
        let status = status.read().unwrap();
        assert_eq!(status.received_count, 1);
        assert_eq!(status.forwarded_count, 1);
        assert_eq!(status.failed_count, 1);
        assert!(status.last_error.contains("Desk"));
    }

    /// 启动一个只接受一次连接、读完完整请求后先睡眠再回 200 的测试服务器，
    /// 用来验证多设备转发是否真的并发执行（而不是排队等前一台超时/变慢）。
    fn slow_test_server(delay: Duration) -> (SocketAddr, thread::JoinHandle<()>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 2048];
            loop {
                let length = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..length]);
                let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                else {
                    continue;
                };
                let header_end = header_end + 4;
                let headers = std::str::from_utf8(&request[..header_end]).unwrap();
                let content_length = headers
                    .split("\r\n")
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                if request.len() >= header_end + content_length {
                    break;
                }
            }
            thread::sleep(delay);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });
        (address, handle)
    }

    #[test]
    fn relay_forwards_to_multiple_devices_concurrently_instead_of_one_at_a_time() {
        let delay = Duration::from_millis(400);
        let (address_one, server_one) = slow_test_server(delay);
        let (address_two, server_two) = slow_test_server(delay);

        let mut second_profile = test_profile();
        second_profile.device_id = "screen-2".to_owned();
        second_profile.slot = 7;
        let data = Arc::new(RwLock::new(SavedMonitorData {
            settings: MonitorSettings {
                base_url: format!("http://{address_one}"),
                username: "Manon".to_owned(),
                device_id: "screen-1".to_owned(),
                device_name: "Desk".to_owned(),
                ..MonitorSettings::default()
            },
            devices: vec![
                MonitorDeviceRoute {
                    base_url: format!("http://{address_one}"),
                    device_id: "screen-1".to_owned(),
                    device_name: "Desk".to_owned(),
                },
                MonitorDeviceRoute {
                    base_url: format!("http://{address_two}"),
                    device_id: "screen-2".to_owned(),
                    device_name: "Studio".to_owned(),
                },
            ],
            profiles: vec![test_profile(), second_profile],
            hook_config_directories: HookConfigDirectories::default(),
        }));
        let status = Arc::new(RwLock::new(HookRelayStatus::default()));
        let online_devices = Arc::new(RwLock::new(Vec::new()));
        // Built and warmed up before starting the clock: constructing a
        // blocking::Client spins up its background runtime, which is a
        // one-off cost unrelated to what this test measures.
        let client = reqwest::blocking::Client::new();

        let started = Instant::now();
        relay_hook(
            &client,
            &data,
            &online_devices,
            &status,
            AiTool::Codex,
            "UserPromptSubmit",
        );
        let elapsed = started.elapsed();

        server_one.join().unwrap();
        server_two.join().unwrap();
        assert_eq!(status.read().unwrap().forwarded_count, 2);
        assert!(
            elapsed < delay * 2,
            "two devices answering in {delay:?} should overlap, not add up (took {elapsed:?})"
        );
    }

    #[test]
    fn terminal_state_guard_suppresses_late_completion_but_not_new_work() {
        let mut terminals = HashMap::new();
        assert!(!should_suppress_late_event(
            &mut terminals,
            AiTool::ClaudeCode,
            "Stop"
        ));
        assert!(should_suppress_late_event(
            &mut terminals,
            AiTool::ClaudeCode,
            "SubagentStop"
        ));
        assert!(!should_suppress_late_event(
            &mut terminals,
            AiTool::ClaudeCode,
            "UserPromptSubmit"
        ));
        assert!(!should_suppress_late_event(
            &mut terminals,
            AiTool::ClaudeCode,
            "PostToolUse"
        ));
    }

    #[test]
    fn switching_current_device_loads_that_devices_profiles() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ai-monitor-route-refresh-{}-{unique}",
            std::process::id()
        ));
        let app_data = root.join("app-data");
        let config_home = root.join("home");
        fs::create_dir_all(&app_data).unwrap();
        let service = MonitorService::load(&app_data, &config_home).unwrap();
        service.save_username("Manon").unwrap();
        service
            .select_device(&DiscoveredMonitorDevice {
                id: "screen-1".to_owned(),
                name: "Desk".to_owned(),
                api_version: "1".to_owned(),
                base_url: "http://192.168.50.10:8080".to_owned(),
                path: "/api/device".to_owned(),
                discovery_source: DiscoverySource::Mdns,
            })
            .unwrap();
        service.save_profile(test_profile()).unwrap();

        service
            .select_device(&DiscoveredMonitorDevice {
                id: "screen-2".to_owned(),
                name: "Studio".to_owned(),
                api_version: "1".to_owned(),
                base_url: "http://192.168.50.99:8080".to_owned(),
                path: "/api/device".to_owned(),
                discovery_source: DiscoverySource::Mdns,
            })
            .unwrap();

        assert!(service.profiles().unwrap().is_empty());
        let mut studio_profile = test_profile();
        studio_profile.slot = 9;
        service.save_profile(studio_profile).unwrap();
        let saved_studio_profile = service.profiles().unwrap().remove(0);
        assert_eq!(saved_studio_profile.device_id, "screen-2");
        assert_eq!(saved_studio_profile.slot, 9);
        service
            .select_device(&DiscoveredMonitorDevice {
                id: "screen-1".to_owned(),
                name: "Desk".to_owned(),
                api_version: "1".to_owned(),
                base_url: "http://192.168.50.10:8080".to_owned(),
                path: "/api/device".to_owned(),
                discovery_source: DiscoverySource::Mdns,
            })
            .unwrap();
        let profile = service.profiles().unwrap().remove(0);
        assert_eq!(profile.tool, AiTool::Codex);
        assert_eq!(profile.device_id, "screen-1");
        assert_eq!(profile.slot, 1);
        assert_eq!(service.settings().unwrap().username, "Manon");
        let saved = service.data.read().unwrap();
        assert_eq!(saved.devices.len(), 2);
        assert!(
            saved
                .devices
                .iter()
                .any(|device| device.device_id == "screen-2")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unavailable_current_device_switches_to_first_online_device_and_persists() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ai-monitor-auto-select-{}-{unique}",
            std::process::id()
        ));
        let app_data = root.join("app-data");
        let config_home = root.join("home");
        fs::create_dir_all(&config_home).unwrap();
        let service = MonitorService::load(&app_data, &config_home).unwrap();
        let current = DiscoveredMonitorDevice {
            id: "screen-1".to_owned(),
            name: "Desk".to_owned(),
            api_version: "1".to_owned(),
            base_url: "http://192.168.50.10:8080".to_owned(),
            path: "/api/device".to_owned(),
            discovery_source: DiscoverySource::Mdns,
        };
        let next = DiscoveredMonitorDevice {
            id: "screen-2".to_owned(),
            name: "Studio".to_owned(),
            api_version: "1".to_owned(),
            base_url: "http://192.168.50.99:8080".to_owned(),
            path: "/api/device".to_owned(),
            discovery_source: DiscoverySource::Mdns,
        };
        service.select_device(&current).unwrap();

        assert!(
            !service
                .select_first_available_device_if_needed(&[])
                .unwrap()
        );
        assert_eq!(service.settings().unwrap().device_id, "screen-1");
        assert!(
            service
                .select_first_available_device_if_needed(std::slice::from_ref(&next))
                .unwrap()
        );
        assert_eq!(service.settings().unwrap().device_id, "screen-2");

        drop(service);
        let reloaded = MonitorService::load(&app_data, &config_home).unwrap();
        assert_eq!(reloaded.settings().unwrap().device_id, "screen-2");
        drop(reloaded);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn batch_image_validation_checks_every_file_before_upload() {
        let images = vec![
            ImageUpload {
                filename: "valid.png".to_owned(),
                mime_type: "image/png".to_owned(),
                bytes: vec![1],
            },
            ImageUpload {
                filename: "invalid.webp".to_owned(),
                mime_type: "image/webp".to_owned(),
                bytes: vec![1],
            },
        ];

        assert_eq!(
            validate_image_uploads(&images),
            Err("invalid.webp 不是支持的 JPEG、PNG 或 GIF 图片".to_owned())
        );
    }

    #[test]
    fn batch_image_validation_rejects_an_empty_selection() {
        assert_eq!(
            validate_image_uploads(&[]),
            Err("请选择要上传的图片".to_owned())
        );
    }

    #[test]
    fn remote_image_url_encodes_one_filename_path_segment() {
        let url = remote_image_url("http://192.168.50.20:8080", "状态 图片 #1.gif").unwrap();

        assert_eq!(
            url.as_str(),
            "http://192.168.50.20:8080/api/images/%E7%8A%B6%E6%80%81%20%E5%9B%BE%E7%89%87%20%231.gif"
        );
        assert!(remote_image_url("http://192.168.50.20:8080", "../secret").is_err());
    }

    #[test]
    fn discovery_prefers_ipv4_candidates_before_ipv6() {
        let mut urls = [
            "http://[fd00::20]:8080".to_owned(),
            "http://192.168.50.20:8080".to_owned(),
        ];

        urls.sort_by_key(|url| candidate_url_priority(url));

        assert_eq!(urls[0], "http://192.168.50.20:8080");
    }

    fn discovery_candidate(
        id: &str,
        name: &str,
        base_url: &str,
        source: DiscoverySource,
    ) -> DiscoveryCandidate {
        DiscoveryCandidate {
            device: DiscoveredMonitorDevice {
                id: id.to_owned(),
                name: name.to_owned(),
                api_version: "1".to_owned(),
                base_url: base_url.to_owned(),
                path: DEFAULT_DEVICE_API_PATH.to_owned(),
                discovery_source: source,
            },
            base_urls: vec![base_url.to_owned()],
        }
    }

    #[test]
    fn merging_discovery_sources_keeps_devices_only_seen_on_one_protocol() {
        // Regression test: two physical devices both running the app, but only
        // one of them answers mDNS reliably (e.g. multicast dropped by the AP)
        // while both answer UDP broadcast. The old short-circuit logic
        // (`if mdns non-empty, ignore udp entirely`) silently dropped the
        // second device from the list.
        let mdns_only = discovery_candidate(
            "device-a",
            "Living Room",
            "http://192.168.1.10:8080",
            DiscoverySource::Mdns,
        );
        let mdns_and_udp = discovery_candidate(
            "device-c",
            "Kitchen",
            "http://192.168.1.30:8080",
            DiscoverySource::Mdns,
        );
        let udp_only = discovery_candidate(
            "device-b",
            "Bedroom",
            "http://192.168.1.20:8080",
            DiscoverySource::UdpBroadcast,
        );
        let mut udp_duplicate_with_extra_address = discovery_candidate(
            "device-c",
            "Kitchen",
            "http://192.168.1.30:8080",
            DiscoverySource::UdpBroadcast,
        );
        udp_duplicate_with_extra_address.base_urls = vec!["http://192.168.1.31:8080".to_owned()];

        let merged = merge_discovery_candidates(
            vec![mdns_only, mdns_and_udp],
            vec![udp_only, udp_duplicate_with_extra_address],
        );

        let ids = merged
            .iter()
            .map(|candidate| candidate.device.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            ids.len(),
            3,
            "mDNS-only, UDP-only and both-source devices must all survive the merge, got {ids:?}"
        );
        assert!(ids.contains(&"device-a"));
        assert!(ids.contains(&"device-b"));

        let kitchen = merged
            .iter()
            .find(|candidate| candidate.device.id == "device-c")
            .unwrap();
        assert_eq!(
            kitchen.base_urls,
            vec![
                "http://192.168.1.30:8080".to_owned(),
                "http://192.168.1.31:8080".to_owned(),
            ],
            "addresses discovered via both protocols for the same device should be unioned"
        );
    }

    #[test]
    fn online_snapshot_requires_two_consecutive_misses_before_removing_a_device() {
        let device_a = discovery_candidate(
            "device-a",
            "Desk",
            "http://192.168.1.10:8080",
            DiscoverySource::Mdns,
        )
        .device;
        let device_b = discovery_candidate(
            "device-b",
            "Studio",
            "http://192.168.1.20:8080",
            DiscoverySource::UdpBroadcast,
        )
        .device;
        let previous = vec![device_a.clone(), device_b.clone()];
        let mut missed_scans = HashMap::new();

        let after_one_miss =
            stabilize_discovered_devices(&previous, vec![device_a.clone()], &mut missed_scans);
        assert_eq!(after_one_miss.len(), 2);

        let after_two_misses = stabilize_discovered_devices(
            &after_one_miss,
            vec![device_a.clone()],
            &mut missed_scans,
        );
        assert_eq!(after_two_misses, vec![device_a]);
        assert_eq!(missed_scans.get("device-b"), Some(&2));
    }

    #[test]
    fn discovery_interval_is_saved_and_read_back_without_restarting_the_service() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ai-monitor-discovery-interval-{}-{unique}",
            std::process::id()
        ));
        let app_data = root.join("app-data");
        let config_home = root.join("home");
        fs::create_dir_all(&app_data).unwrap();
        let service = MonitorService::load(&app_data, &config_home).unwrap();

        assert_eq!(service.discovery_interval(), Duration::from_mins(1));
        assert!(service.save_discovery_interval(0).is_err());

        let updated = service.save_discovery_interval(15).unwrap();
        assert_eq!(updated.discovery_interval_minutes, 15);
        assert_eq!(service.discovery_interval(), Duration::from_mins(15));
    }

    #[test]
    fn discovery_formats_addresses_for_direct_health_probes() {
        let ipv4 = ScopedIp::V4(ScopedIpV4::new(
            Ipv4Addr::new(192, 168, 50, 20),
            InterfaceId {
                name: "Ethernet".to_owned(),
                index: 12,
            },
        ));
        let ipv6 = ScopedIp::from(IpAddr::V6("fd00::20".parse::<Ipv6Addr>().unwrap()));

        assert_eq!(
            discovery_base_url(&ipv4, 8080).as_deref(),
            Some("http://192.168.50.20:8080")
        );
        assert_eq!(
            discovery_base_url(&ipv6, 8080).as_deref(),
            Some("http://[fd00::20]:8080")
        );
    }

    #[test]
    fn directed_broadcast_uses_the_interface_netmask() {
        assert_eq!(
            directed_broadcast(
                Ipv4Addr::new(192, 168, 50, 20),
                Ipv4Addr::new(255, 255, 255, 0)
            ),
            Ipv4Addr::new(192, 168, 50, 255)
        );
        assert_eq!(
            directed_broadcast(Ipv4Addr::new(10, 23, 45, 67), Ipv4Addr::new(255, 255, 0, 0)),
            Ipv4Addr::new(10, 23, 255, 255)
        );
    }

    #[test]
    fn udp_response_uses_the_datagram_source_ip_and_advertised_port() {
        let device = parse_udp_discovery_response(
            r#"{"id":"device-17","name":"客厅监控屏","port":8080,"apiVersion":"1"}"#.as_bytes(),
            "192.168.50.23:49152".parse().unwrap(),
        )
        .unwrap();

        assert_eq!(device.id, "device-17");
        assert_eq!(device.name, "客厅监控屏");
        assert_eq!(device.base_url, "http://192.168.50.23:8080");
        assert_eq!(device.api_version, "1");
        assert_eq!(device.discovery_source, DiscoverySource::UdpBroadcast);
    }

    #[test]
    fn udp_response_rejects_invalid_metadata() {
        assert!(
            parse_udp_discovery_response(
                br#"{"id":"","name":"Monitor","port":8080,"apiVersion":"1"}"#,
                "192.168.50.23:49152".parse().unwrap(),
            )
            .is_none()
        );
        assert!(
            parse_udp_discovery_response(
                br#"{"id":"device","name":"Monitor","port":0,"apiVersion":"1"}"#,
                "192.168.50.23:49152".parse().unwrap(),
            )
            .is_none()
        );
    }

    #[test]
    fn udp_discovery_round_trip_matches_the_android_protocol() {
        let server = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let port = server.local_addr().unwrap().port();
        let responder = thread::spawn(move || {
            let mut request = [0_u8; 256];
            let (length, source) = server.recv_from(&mut request).unwrap();
            assert_eq!(&request[..length], UDP_DISCOVERY_REQUEST);
            server
                .send_to(
                    r#"{"id":"device-loopback","name":"测试设备","port":8080,"apiVersion":"1"}"#
                        .as_bytes(),
                    source,
                )
                .unwrap();
        });

        let candidates = discover_udp_on_targets(
            &[UdpBroadcastTarget {
                local_ip: Ipv4Addr::LOCALHOST,
                broadcast_ip: Ipv4Addr::LOCALHOST,
            }],
            port,
            Duration::from_millis(250),
        )
        .unwrap();
        responder.join().unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].device.id, "device-loopback");
        assert_eq!(candidates[0].device.base_url, "http://127.0.0.1:8080");
    }

    #[test]
    fn profile_save_does_not_write_hooks_when_data_persistence_fails() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ai-monitor-hook-transaction-{}-{unique}",
            std::process::id()
        ));
        let config_home = root.join("home");
        let invalid_data_path = root.join("data-path-is-a-directory");
        fs::create_dir_all(&invalid_data_path).unwrap();

        let service = MonitorService {
            client: Client::new(),
            data_path: invalid_data_path,
            default_hook_config_directories: HookConfigDirectories {
                codex: config_home.join(".codex").to_string_lossy().into_owned(),
                claude_code: config_home.join(".claude").to_string_lossy().into_owned(),
                cursor: config_home.join(".cursor").to_string_lossy().into_owned(),
            },
            data: Arc::new(RwLock::new(SavedMonitorData {
                settings: MonitorSettings {
                    base_url: "http://127.0.0.1:8080".to_owned(),
                    username: "tester".to_owned(),
                    device_id: "device-1".to_owned(),
                    device_name: "monitor".to_owned(),
                    ..MonitorSettings::default()
                },
                devices: Vec::new(),
                profiles: Vec::new(),
                hook_config_directories: HookConfigDirectories::default(),
            })),
            online_devices: Arc::new(RwLock::new(Vec::new())),
            discovery_missed_scans: Arc::new(Mutex::new(HashMap::new())),
            hook_config_write_lock: Arc::new(Mutex::new(())),
            relay_status: Arc::new(RwLock::new(HookRelayStatus::default())),
        };

        let result = service.save_profile(test_profile());

        assert!(result.is_err());
        assert!(!config_home.join(".codex/hooks.json").exists());
        assert!(service.profiles().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn custom_hook_directory_is_persisted_and_used_for_hook_writes() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ai-monitor-hook-directory-{}-{unique}",
            std::process::id()
        ));
        let app_data = root.join("app-data");
        let config_home = root.join("home");
        fs::create_dir_all(&app_data).unwrap();
        let service = MonitorService::load(&app_data, &config_home).unwrap();
        service
            .select_device(&DiscoveredMonitorDevice {
                id: "screen-1".to_owned(),
                name: "Desk".to_owned(),
                api_version: "1".to_owned(),
                base_url: "http://127.0.0.1:8080".to_owned(),
                path: "/api/device".to_owned(),
                discovery_source: DiscoverySource::Mdns,
            })
            .unwrap();
        let custom_directory = root.join("custom-codex");
        let detected_directory = service
            .hook_config_locations()
            .unwrap()
            .into_iter()
            .find(|item| item.tool == AiTool::Codex)
            .unwrap()
            .directory;

        let location = service
            .save_hook_config_directory(AiTool::Codex, &custom_directory.to_string_lossy())
            .unwrap();

        assert!(location.is_custom);
        assert_eq!(
            PathBuf::from(&location.config_path),
            custom_directory.join("hooks.json")
        );
        service.save_profile(test_profile()).unwrap();
        assert!(!custom_directory.join("hooks.json").exists());
        service.write_hook_config(AiTool::Codex).unwrap();
        assert!(custom_directory.join("hooks.json").exists());
        assert!(!config_home.join(".codex/hooks.json").exists());
        let reloaded = MonitorService::load(&app_data, &config_home).unwrap();
        let reloaded_location = reloaded
            .hook_config_locations()
            .unwrap()
            .into_iter()
            .find(|item| item.tool == AiTool::Codex)
            .unwrap();
        assert_eq!(reloaded_location.directory, location.directory);
        assert!(reloaded_location.is_custom);

        let default_location = reloaded
            .save_hook_config_directory(AiTool::Codex, "")
            .unwrap();
        assert!(!default_location.is_custom);
        assert_eq!(default_location.directory, detected_directory);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hook_directory_rejects_relative_and_file_paths() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ai-monitor-hook-directory-validation-{}-{unique}",
            std::process::id()
        ));
        let app_data = root.join("app-data");
        let config_home = root.join("home");
        fs::create_dir_all(&app_data).unwrap();
        let service = MonitorService::load(&app_data, &config_home).unwrap();
        let file_path = root.join("not-a-directory");
        fs::write(&file_path, "content").unwrap();

        assert!(
            service
                .save_hook_config_directory(AiTool::Cursor, "relative/path")
                .is_err()
        );
        assert!(
            service
                .save_hook_config_directory(AiTool::ClaudeCode, &file_path.to_string_lossy())
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
