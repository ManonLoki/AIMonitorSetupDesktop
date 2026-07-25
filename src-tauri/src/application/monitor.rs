use std::{
    collections::{HashMap, HashSet},
    fs,
    io::ErrorKind,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    path::{Path, PathBuf},
    sync::RwLock,
    thread,
    time::{Duration, Instant},
};

use if_addrs::{IfAddr, IfOperStatus};
use mdns_sd::{ScopedIp, ServiceDaemon, ServiceEvent};
use reqwest::{Client, StatusCode, Url, header, multipart};
use serde::{Deserialize, Serialize};

use crate::domain::monitor::{
    AiProfile, AiTool, DiscoveredMonitorDevice, DiscoverySource, HookConfigDirectories,
    HookConfigLocation, HookConfigPreview, HookConfigWriteResult, HookRunnerPaths, LocalHookConfig,
    MonitorSettings, SavedMonitorData, encode_base64, generate_hook_config,
    generate_hook_runner_scripts, hook_config_filename, inspect_local_hook_config,
    merge_hook_config, migrate_legacy_profile, normalize_base_url, validate_profile,
    validate_settings,
};

const STORE_FILENAME: &str = "monitor-data.json";
const POSIX_RUNNER_FILENAME: &str = "aimonitor-hook.sh";
const WINDOWS_RUNNER_FILENAME: &str = "aimonitor-hook.ps1";
const AIMONITOR_SERVICE_TYPE: &str = "_aimonitor._tcp.local.";
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(4);
const DISCOVERY_PROBE_TIMEOUT: Duration = Duration::from_millis(900);
const UDP_DISCOVERY_PORT: u16 = 8080;
const UDP_DISCOVERY_REQUEST: &[u8] = b"AIMONITOR_DISCOVER_V1";
const UDP_DISCOVERY_TIMEOUT: Duration = Duration::from_millis(1_200);
const UDP_RESPONSE_MAX_BYTES: usize = 1_024;
const DEFAULT_DEVICE_API_PATH: &str = "/api/device";
const MAX_REMOTE_IMAGE_BYTES: usize = 8 * 1024 * 1024;

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

pub struct MonitorService {
    client: Client,
    data_path: PathBuf,
    default_hook_config_directories: HookConfigDirectories,
    runner_paths: HookRunnerPaths,
    data: RwLock<SavedMonitorData>,
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
        let mut data = if data_path.exists() {
            let contents =
                fs::read_to_string(&data_path).map_err(|error| format!("无法读取配置：{error}"))?;
            serde_json::from_str(&contents).map_err(|error| format!("配置文件格式错误：{error}"))?
        } else {
            SavedMonitorData::default()
        };
        for profile in &mut data.profiles {
            migrate_legacy_profile(profile);
        }

        Ok(Self {
            client: Client::new(),
            data_path,
            default_hook_config_directories: detect_hook_config_directories(config_home),
            runner_paths: HookRunnerPaths {
                posix: app_data_dir
                    .join(POSIX_RUNNER_FILENAME)
                    .to_string_lossy()
                    .into_owned(),
                windows: app_data_dir
                    .join(WINDOWS_RUNNER_FILENAME)
                    .to_string_lossy()
                    .into_owned(),
            },
            data: RwLock::new(data),
        })
    }

    pub fn settings(&self) -> Result<MonitorSettings, String> {
        self.data
            .read()
            .map(|data| data.settings.clone())
            .map_err(|_| "配置读取锁已损坏".to_owned())
    }

    pub fn save_settings(
        &self,
        device: &DiscoveredMonitorDevice,
        username: &str,
    ) -> Result<MonitorSettings, String> {
        let settings = validate_settings(device, username)?;
        let mut data = self
            .data
            .write()
            .map_err(|_| "配置写入锁已损坏".to_owned())?;
        let mut next_data = data.clone();
        next_data.settings = settings.clone();
        self.persist_with_runners(&next_data)?;
        *data = next_data;
        Ok(settings)
    }

    /// 设备发现的主入口：优先使用 mDNS（更快、支持局域网内跨网段发现），
    /// 找不到结果或出错时回退到向每张网卡发送 UDP 广播。两者都失败才报错。
    pub(crate) fn discover_device_candidates() -> Result<Vec<DiscoveryCandidate>, String> {
        match Self::discover_mdns_candidates() {
            Ok(candidates) if !candidates.is_empty() => Ok(candidates),
            Ok(_) => discover_udp_candidates(),
            Err(mdns_error) => discover_udp_candidates().map_err(|udp_error| {
                format!("mDNS 发现失败：{mdns_error}；UDP 广播发现失败：{udp_error}")
            }),
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
            .map(|data| data.profiles.clone())
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

    pub fn write_profile(&self, profile: AiProfile) -> Result<HookConfigWriteResult, String> {
        let profile = validate_profile(profile)?;
        let mut data = self
            .data
            .write()
            .map_err(|_| "AI 配置写入锁已损坏".to_owned())?;
        let generated = generate_hook_config(profile.clone(), &self.runner_paths)?;
        let config_path = self.hook_config_path(&data, profile.tool);
        let existing = read_optional_config(&config_path)?;
        let mut merged = merge_hook_config(existing.as_deref(), &generated, profile.tool)?;
        merged.filename = config_path.to_string_lossy().into_owned();
        let config_changed = existing.as_deref() != Some(merged.content.as_str());

        let mut next_data = data.clone();
        next_data
            .profiles
            .retain(|existing| existing.tool != profile.tool);
        next_data.profiles.push(profile.clone());
        next_data.profiles.sort_by_key(|item| item.slot);

        self.persist_profile_transaction(
            &next_data,
            &config_path,
            existing.as_deref(),
            config_changed.then_some(merged.content.as_str()),
        )?;
        *data = next_data;

        Ok(HookConfigWriteResult {
            requires_review: profile.tool == AiTool::Codex && config_changed,
            restart_required: profile.tool == AiTool::Codex && config_changed,
            profile,
            filename: merged.filename,
            config_changed,
        })
    }

    pub fn hook_config_preview(&self, profile: AiProfile) -> Result<HookConfigPreview, String> {
        let tool = profile.tool;
        let generated = generate_hook_config(profile, &self.runner_paths)?;
        let data = self
            .data
            .read()
            .map_err(|_| "Hooks 路径读取锁已损坏".to_owned())?;
        let config_path = self.hook_config_path(&data, tool);
        let existing = read_optional_config(&config_path)?;
        let mut merged = merge_hook_config(existing.as_deref(), &generated, tool)?;
        merged.filename = config_path.to_string_lossy().into_owned();
        Ok(merged)
    }

    pub fn local_hook_configs(&self) -> Result<Vec<LocalHookConfig>, String> {
        let data = self
            .data
            .read()
            .map_err(|_| "Hooks 路径读取锁已损坏".to_owned())?;
        AiTool::ALL
            .into_iter()
            .map(|tool| {
                let config_path = self.hook_config_path(&data, tool);
                let content = read_optional_config(&config_path)?;
                Ok(inspect_local_hook_config(
                    tool,
                    config_path.to_string_lossy().into_owned(),
                    content,
                ))
            })
            .collect()
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

    fn persist_with_runners(&self, data: &SavedMonitorData) -> Result<(), String> {
        let scripts = generate_hook_runner_scripts(&data.settings, &data.profiles)?;
        let posix_path = Path::new(&self.runner_paths.posix);
        let windows_path = Path::new(&self.runner_paths.windows);
        let previous_posix = read_optional_config(posix_path)?;
        let previous_windows = read_optional_config(windows_path)?;

        write_atomic_file(posix_path, &scripts.posix, "Hook 运行脚本")?;
        if let Err(error) =
            write_atomic_file(windows_path, &scripts.windows, "Windows Hook 运行脚本")
        {
            let _ = restore_optional_file(posix_path, previous_posix.as_deref(), "Hook 运行脚本");
            return Err(error);
        }
        if let Err(error) = self.persist(data) {
            let rollback = rollback_runner_files(
                posix_path,
                previous_posix.as_deref(),
                windows_path,
                previous_windows.as_deref(),
            );
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback_error) => {
                    Err(format!("{error}；Hook 运行脚本回滚失败：{rollback_error}"))
                }
            };
        }
        Ok(())
    }

    fn persist_profile_transaction(
        &self,
        data: &SavedMonitorData,
        config_path: &Path,
        previous_config: Option<&str>,
        next_config: Option<&str>,
    ) -> Result<(), String> {
        if let Some(next_config) = next_config {
            write_config(config_path, next_config)?;
        }
        if let Err(error) = self.persist_with_runners(data) {
            if next_config.is_some()
                && let Err(rollback_error) = restore_optional_config(config_path, previous_config)
            {
                return Err(format!(
                    "{error}；Hooks 配置回滚失败，当前文件可能已发生变化：{rollback_error}"
                ));
            }
            return Err(error);
        }
        Ok(())
    }
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

fn restore_optional_config(path: &Path, content: Option<&str>) -> Result<(), String> {
    restore_optional_file(path, content, "Hooks 配置")
}

fn restore_optional_file(path: &Path, content: Option<&str>, label: &str) -> Result<(), String> {
    if let Some(content) = content {
        return write_atomic_file(path, content, label);
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("无法删除新建配置 {}：{error}", path.display())),
    }
}

fn rollback_runner_files(
    posix_path: &Path,
    previous_posix: Option<&str>,
    windows_path: &Path,
    previous_windows: Option<&str>,
) -> Result<(), String> {
    let posix_result = restore_optional_file(posix_path, previous_posix, "Hook 运行脚本");
    let windows_result =
        restore_optional_file(windows_path, previous_windows, "Windows Hook 运行脚本");
    posix_result.and(windows_result)
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
    fn profile_write_rolls_back_hook_file_when_data_persistence_fails() {
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
            runner_paths: HookRunnerPaths {
                posix: root
                    .join(POSIX_RUNNER_FILENAME)
                    .to_string_lossy()
                    .into_owned(),
                windows: root
                    .join(WINDOWS_RUNNER_FILENAME)
                    .to_string_lossy()
                    .into_owned(),
            },
            data: RwLock::new(SavedMonitorData {
                settings: MonitorSettings {
                    base_url: "http://127.0.0.1:8080".to_owned(),
                    username: "tester".to_owned(),
                    device_id: "device-1".to_owned(),
                    device_name: "monitor".to_owned(),
                },
                profiles: Vec::new(),
                hook_config_directories: HookConfigDirectories::default(),
            }),
        };

        let result = service.write_profile(test_profile());

        assert!(result.is_err());
        assert!(!config_home.join(".codex/hooks.json").exists());
        assert!(service.profiles().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn custom_hook_directory_is_persisted_and_used_for_profile_writes() {
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
        service.write_profile(test_profile()).unwrap();
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
