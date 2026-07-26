// 标准库依赖：集合、文件系统、IO 读写、网络套接字（TCP/UDP）、路径、
// 线程安全共享（Arc/Mutex/RwLock/mpsc 通道）、线程、时间。
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

// 第三方依赖：网卡信息枚举（if_addrs）、mDNS 服务发现（mdns_sd）、
// HTTP 客户端与 multipart 上传（reqwest）、序列化（serde）、
// Tauri 应用句柄与事件发送（tauri）。
use if_addrs::{IfAddr, IfOperStatus};
use mdns_sd::{ScopedIp, ServiceDaemon, ServiceEvent};
use reqwest::{Client, StatusCode, Url, header, multipart};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

// 引入领域层（domain/monitor.rs）的实体类型与纯业务函数，
// 本文件（application 层）只做编排，具体业务规则在 domain 层实现。
use crate::domain::monitor::{
    AiProfile, AiTool, DEFAULT_HOOK_RELAY_PORT, DiscoveredMonitorDevice, DiscoverySource,
    HookBehavior, HookConfigDirectories, HookConfigLocation, HookConfigWriteResult,
    HookEventDecision, HookStateMachine, HookTransition, MonitorDeviceRoute, MonitorSettings,
    SavedMonitorData, ai_tool_name, encode_base64, generate_hook_auxiliary_configs,
    generate_hook_config, hook_config_filename, hook_requires_review, hook_restart_required,
    merge_hook_config, normalize_base_url, normalize_enabled_ai_tools, process_image_upload,
    tool_from_slug, validate_device_route, validate_discovery_interval_minutes, validate_profile,
    validate_saved_monitor_data, validate_username,
};

// 本地持久化存储文件名（保存监控配置数据的 JSON 文件）。
const STORE_FILENAME: &str = "monitor-data.json";
// mDNS 服务发现使用的服务类型标识。
const AIMONITOR_SERVICE_TYPE: &str = "_aimonitor._tcp.local.";
// 整体发现流程的超时时间。
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
// 对单个候选地址做连接探测时的超时时间。
const DISCOVERY_PROBE_TIMEOUT: Duration = Duration::from_millis(900);
// UDP 广播发现所使用的端口。
const UDP_DISCOVERY_PORT: u16 = 8080;
// UDP 广播发现请求的固定报文内容。
const UDP_DISCOVERY_REQUEST: &[u8] = b"AIMONITOR_DISCOVER_V1";
// 等待 UDP 广播响应的超时时间。
const UDP_DISCOVERY_TIMEOUT: Duration = Duration::from_millis(1_200);
// 单个 UDP 响应报文允许的最大字节数，超出则丢弃。
const UDP_RESPONSE_MAX_BYTES: usize = 1_024;
// 设备 HTTP API 的默认路径前缀。
const DEFAULT_DEVICE_API_PATH: &str = "/api/device";
// 允许下载的远程图片最大字节数（8MB），超过则拒绝。
const MAX_REMOTE_IMAGE_BYTES: usize = 8 * 1024 * 1024;
// 本机 Hook 中继监听端口，直接复用领域层的默认端口常量。
pub const HOOK_LISTENER_PORT: u16 = DEFAULT_HOOK_RELAY_PORT;
// Hook 中继 TCP 监听器只绑定在本机回环地址，不对外暴露。
const HOOK_BIND_ADDRESS: &str = "127.0.0.1";
// 原始 Hook JSON 可能包含用户 prompt、工具输入/输出等上下文；限制为 4 MiB，
// 既允许完整转发常规事件，又避免异常本机连接无界占用内存。
const MAX_HOOK_BODY_BYTES: usize = 4 * 1024 * 1024;
// 完整 HTTP 请求额外为请求头预留 8 KiB。
const MAX_HOOK_REQUEST_BYTES: usize = MAX_HOOK_BODY_BYTES + 8 * 1024;
// listener 到状态机 worker 的队列使用固定容量；状态机推进不再等待设备网络，
// 正常情况下会很快腾出空间，极端洪峰则通过短暂背压保护进程内存。
const HOOK_EVENT_QUEUE_CAPACITY: usize = 256;
// 每个工具在自己的投递 worker 前最多只需要一个唤醒令牌：若该工具已经有
// 一个待发送的最新状态，后续事件只覆盖 mailbox，不再重复排队唤醒。
const HOOK_RELAY_WAKE_QUEUE_CAPACITY: usize = 1;
// 会话长时间没有任何事件时视为孤儿并回收。新事件仍可按当前事件语义重新建立
// 隐式会话，因此超时不会让后续真实状态永久丢失。
const HOOK_SESSION_INACTIVITY_TIMEOUT: Duration = Duration::from_mins(30);
// 即使没有新 Hook，也按此粒度清扫一次超时会话。
const HOOK_SESSION_SWEEP_INTERVAL: Duration = Duration::from_secs(1);
// 会进入队列或状态机长期保存的上下文字段使用独立上限，避免单个合法 4 MiB
// JSON 里的超长标识在洪峰时放大内存占用。
const MAX_HOOK_SESSION_ID_BYTES: usize = 512;
const MAX_HOOK_TURN_ID_BYTES: usize = 512;
const MAX_HOOK_STATUS_BYTES: usize = 64;
// 连续多少次发现轮询未命中某设备后，才将其判定为离线并移除。
const DISCOVERY_MISSES_BEFORE_REMOVAL: u8 = 2;
/// 后台发现循环的轮询粒度：每次醒来都会重新读取当前配置的检查间隔，
/// 因此设置页修改间隔后，最多这么久就会生效，无需重启线程。
const DISCOVERY_POLL_GRANULARITY: Duration = Duration::from_secs(1);
// 设备列表发生变化时向前端发送的 Tauri 事件名称。
pub const MONITOR_DEVICES_CHANGED_EVENT: &str = "monitor-devices-changed";

// 根据用户主目录与工具公开的环境变量，探测各 AI 工具的 Hook 配置目录。
fn detect_hook_config_directories(config_home: &Path) -> HookConfigDirectories {
    let open_code_fallback = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| config_home.join(".config"))
        .join("opencode");
    HookConfigDirectories {
        // Codex 配置目录：优先读取环境变量 CODEX_HOME，否则回退到 ~/.codex。
        codex: detected_config_directory("CODEX_HOME", &config_home.join(".codex")),
        // Claude Code 配置目录：优先读取环境变量 CLAUDE_CONFIG_DIR，否则回退到 ~/.claude。
        claude_code: detected_config_directory("CLAUDE_CONFIG_DIR", &config_home.join(".claude")),
        // Cursor 配置目录固定为 ~/.cursor（没有对应的环境变量覆盖）。
        cursor: config_home.join(".cursor").to_string_lossy().into_owned(),
        // OpenCode 支持 OPENCODE_CONFIG_DIR；否则遵循 XDG_CONFIG_HOME/opencode。
        open_code: detected_config_directory("OPENCODE_CONFIG_DIR", &open_code_fallback),
        // WorkBuddy 自 v2.48 起与 CodeBuddy CLI 分离，使用 ~/.workbuddy。
        work_buddy: config_home
            .join(".workbuddy")
            .to_string_lossy()
            .into_owned(),
        // Harness 支持 HARNESS_HOME；默认位置按其 macOS/Linux 公开路径选择。
        harness: detected_config_directory("HARNESS_HOME", &default_harness_home(config_home)),
        // OpenClaw 的可变状态目录可由 OPENCLAW_STATE_DIR 覆盖。
        open_claw: detected_config_directory("OPENCLAW_STATE_DIR", &config_home.join(".openclaw")),
        // CodeBuddy 官方通过 CODEBUDDY_CONFIG_DIR 覆盖 ~/.codebuddy。
        code_buddy: detected_config_directory(
            "CODEBUDDY_CONFIG_DIR",
            &config_home.join(".codebuddy"),
        ),
    }
}

#[cfg(target_os = "macos")]
fn default_harness_home(config_home: &Path) -> PathBuf {
    config_home.join("Library/Application Support/Harness")
}

#[cfg(not(target_os = "macos"))]
fn default_harness_home(config_home: &Path) -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| config_home.join(".local/share"))
        .join("harness")
}

// 读取指定环境变量作为配置目录，若变量不存在或不是绝对路径则使用回退路径。
fn detected_config_directory(variable: &str, fallback: &Path) -> String {
    std::env::var_os(variable)
        .map(PathBuf::from)
        // 只信任绝对路径的环境变量值，避免相对路径造成歧义。
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| fallback.to_owned())
        .to_string_lossy()
        .into_owned()
}

/// 从系统环境与主目录名中取得当前本机用户名。环境变量在桌面启动环境缺失时，
/// 主目录末级名称仍可覆盖 macOS/Linux/Windows 的常见用户目录布局。
fn detect_system_username(config_home: &Path) -> Option<String> {
    ["USER", "USERNAME", "LOGNAME"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .chain(
            config_home
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned),
        )
        .find_map(|candidate| validate_username(&candidate).ok())
}

/// 一次发现命中的设备，可能同时拥有多个候选地址（IPv4/IPv6、多网卡）；
/// 连接测试按 `candidate_url_priority` 排序后依次尝试，取第一个可达的。
#[derive(Clone, Debug)]
pub(crate) struct DiscoveryCandidate {
    // 已发现的设备领域实体。
    device: DiscoveredMonitorDevice,
    // 该设备所有候选的 base url（按优先级排序前的原始集合）。
    base_urls: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct UdpBroadcastTarget {
    // 本机用于发送广播的网卡 IPv4 地址。
    local_ip: Ipv4Addr,
    // 该网卡对应的广播地址。
    broadcast_ip: Ipv4Addr,
}

// UDP 发现响应报文的反序列化结构，字段名按 camelCase 与设备端 JSON 对应。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UdpDiscoveryResponse {
    id: String,
    name: String,
    port: u16,
    api_version: String,
}

// 根据 mDNS 返回的作用域地址与端口，拼出可用于 HTTP 请求的 base url。
fn discovery_base_url(address: &ScopedIp, port: u16) -> Option<String> {
    match address {
        // IPv4 地址直接拼接。
        ScopedIp::V4(address) => Some(format!("http://{}:{port}", address.addr())),
        // IPv6 链路本地地址必须带上作用域 ID（zone id），否则无法正确路由；
        // 若作用域 ID 为 0（无效），则放弃该候选地址。
        ScopedIp::V6(address) if address.addr().is_unicast_link_local() => {
            let scope_id = address.scope_id().index;
            (scope_id != 0).then(|| format!("http://[{}%25{scope_id}]:{port}", address.addr()))
        }
        // 其他 IPv6 地址（非链路本地）直接拼接，无需作用域 ID。
        ScopedIp::V6(address) => Some(format!("http://[{}]:{port}", address.addr())),
        // 未知地址类型不生成候选。
        _ => None,
    }
}

/// IPv4 地址优先于 IPv6（`http://[` 形式，第 8 个字符是 `[`）：IPv6
/// 链路本地地址更容易受网卡切换、作用域 ID 失效等问题影响，连接稳定性更低。
fn candidate_url_priority(base_url: &str) -> u8 {
    // 第 8 个字节（下标 7）是 `[` 说明是 IPv6 字面量地址（http://[...），返回 1（低优先级）；
    // IPv4 或其他情况返回 0（高优先级），从而让排序时 IPv4 排在前面。
    u8::from(base_url.as_bytes().get(7) == Some(&b'['))
}

// 通过 UDP 广播方式发现设备：先枚举本机所有可用网卡的广播目标，再逐个发送探测报文。
fn discover_udp_candidates() -> Result<Vec<DiscoveryCandidate>, String> {
    let targets = udp_broadcast_targets()?;
    discover_udp_on_targets(&targets, UDP_DISCOVERY_PORT, UDP_DISCOVERY_TIMEOUT)
}

// 枚举本机网卡，计算出每个可用网卡对应的 UDP 广播目标地址。
fn udp_broadcast_targets() -> Result<Vec<UdpBroadcastTarget>, String> {
    // 获取本机所有网卡地址信息。
    let interfaces =
        if_addrs::get_if_addrs().map_err(|error| format!("无法枚举本机网卡：{error}"))?;
    let mut targets = interfaces
        .into_iter()
        // 只保留处于 Up 或状态未知（部分平台不会上报明确状态）的网卡，
        // 排除回环接口和点对点接口（它们不适合做局域网广播发现）。
        .filter(|interface| {
            matches!(
                interface.oper_status,
                IfOperStatus::Up | IfOperStatus::Unknown
            ) && !interface.is_loopback()
                && !interface.is_p2p()
        })
        // 只处理 IPv4 且非未指定地址（0.0.0.0）的网卡，计算其广播地址。
        .filter_map(|interface| match interface.addr {
            IfAddr::V4(address) if !address.ip.is_unspecified() => {
                // 优先使用系统上报的广播地址；若未提供则根据 IP+子网掩码手动计算定向广播地址。
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
    // 排序后去重，避免同一网卡/广播地址组合被重复探测。
    targets.sort_by_key(|target| (target.local_ip, target.broadcast_ip));
    targets.dedup();

    // 如果没有枚举到任何可用网卡（极端情况），退化为使用全局广播地址兜底。
    if targets.is_empty() {
        targets.push(UdpBroadcastTarget {
            local_ip: Ipv4Addr::UNSPECIFIED,
            broadcast_ip: Ipv4Addr::BROADCAST,
        });
    }
    Ok(targets)
}

// 根据 IP 与子网掩码计算定向广播地址：IP 与掩码取反后按位或。
fn directed_broadcast(ip: Ipv4Addr, netmask: Ipv4Addr) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(ip) | !u32::from(netmask))
}

// 在给定的多个广播目标上并发发送 UDP 探测报文并收集响应，汇总为发现候选列表。
fn discover_udp_on_targets(
    targets: &[UdpBroadcastTarget],
    discovery_port: u16,
    timeout: Duration,
) -> Result<Vec<DiscoveryCandidate>, String> {
    // 为每个广播目标准备一个 UDP socket，同时记录绑定失败的错误信息。
    let mut sockets = Vec::with_capacity(targets.len());
    let mut bind_errors = Vec::new();

    for target in targets {
        // 绑定到该网卡本地地址的随机端口（0 表示由系统分配）。
        match UdpSocket::bind(SocketAddrV4::new(target.local_ip, 0)) {
            Ok(socket) => {
                // 允许发送广播报文。
                if let Err(error) = socket.set_broadcast(true) {
                    bind_errors.push(format!("{}：{error}", target.local_ip));
                    continue;
                }
                // 设置非阻塞模式，便于后续轮询接收多个 socket。
                if let Err(error) = socket.set_nonblocking(true) {
                    bind_errors.push(format!("{}：{error}", target.local_ip));
                    continue;
                }
                sockets.push((socket, *target));
            }
            Err(error) => bind_errors.push(format!("{}：{error}", target.local_ip)),
        }
    }

    // 所有网卡都绑定失败时直接返回错误，携带每个网卡的失败原因。
    if sockets.is_empty() {
        return Err(format!(
            "无法在任何 IPv4 网卡上创建 UDP socket：{}",
            bind_errors.join("；")
        ));
    }

    // 发送两轮探测报文（间隔 75ms），提高在丢包网络下的命中率。
    for _ in 0..2 {
        for (socket, target) in &sockets {
            // 同时向该网卡的定向广播地址和全局广播地址发送，去重后避免重复发送同一地址。
            let destinations = [target.broadcast_ip, Ipv4Addr::BROADCAST]
                .into_iter()
                .collect::<HashSet<_>>();
            for destination in destinations {
                // 发送失败直接忽略（尽力而为，不影响其他网卡的探测）。
                let _ = socket.send_to(
                    UDP_DISCOVERY_REQUEST,
                    SocketAddrV4::new(destination, discovery_port),
                );
            }
        }
        thread::sleep(Duration::from_millis(75));
    }

    // 计算总体等待截止时间，用设备 id 去重合并候选结果。
    let deadline = Instant::now() + timeout;
    let mut candidates = HashMap::<String, DiscoveryCandidate>::new();
    let mut response = [0_u8; UDP_RESPONSE_MAX_BYTES];

    while Instant::now() < deadline {
        let mut received_any = false;
        // 轮询每个 socket，非阻塞地读取所有已到达的响应报文。
        for (socket, _) in &sockets {
            loop {
                match socket.recv_from(&mut response) {
                    Ok((length, source)) => {
                        received_any = true;
                        // 解析成功则合并进候选集合（同一设备可能从多网卡收到响应）。
                        if let Some(device) =
                            parse_udp_discovery_response(&response[..length], source)
                        {
                            merge_udp_candidate(&mut candidates, device);
                        }
                    }
                    // 非阻塞 socket 无数据可读时会返回 WouldBlock，跳出内层循环换下一个 socket。
                    Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }
        }
        // 本轮没有收到任何数据时短暂休眠，避免忙等占满 CPU。
        if !received_any {
            thread::sleep(Duration::from_millis(10));
        }
    }

    // 按设备名称排序后返回，保证结果顺序稳定。
    let mut candidates = candidates.into_values().collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.device.name.cmp(&right.device.name));
    Ok(candidates)
}

// 解析单个 UDP 发现响应报文，校验来源地址与字段合法性后转换为设备实体。
fn parse_udp_discovery_response(
    bytes: &[u8],
    source: SocketAddr,
) -> Option<DiscoveredMonitorDevice> {
    // 只接受 IPv4 来源（UDP 发现协议本身只在 IPv4 网段广播）。
    let source_ip = match source {
        SocketAddr::V4(source) => *source.ip(),
        SocketAddr::V6(_) => return None,
    };
    // 过滤掉非法/伪造的来源地址（未指定、组播、广播地址都不可能是真实设备）。
    if source_ip.is_unspecified() || source_ip.is_multicast() || source_ip.is_broadcast() {
        return None;
    }

    // 反序列化 JSON 报文，失败则视为无效响应。
    let response = serde_json::from_slice::<UdpDiscoveryResponse>(bytes).ok()?;
    let id = response.id.trim();
    let name = response.name.trim();
    let api_version = response.api_version.trim();
    // 校验各字段非空且长度在合理范围内，端口不能为 0，否则丢弃该响应。
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

    // 校验通过后构造设备实体，base_url 直接使用来源 IP 和响应中的端口拼出。
    Some(DiscoveredMonitorDevice {
        id: id.to_owned(),
        name: name.to_owned(),
        api_version: api_version.to_owned(),
        base_url: format!("http://{source_ip}:{}", response.port),
        path: DEFAULT_DEVICE_API_PATH.to_owned(),
        discovery_source: DiscoverySource::UdpBroadcast,
    })
}

// 将单个 UDP 发现到的设备合并进候选表：同一设备 id 若已存在则追加新的 base_url，
// 否则新建一条候选记录；每次合并后都按优先级重新排序候选地址。
fn merge_udp_candidate(
    candidates: &mut HashMap<String, DiscoveryCandidate>,
    device: DiscoveredMonitorDevice,
) {
    let base_url = device.base_url.clone();
    // 若该设备 id 已在候选表中则取出已有记录，否则插入一条新记录。
    let candidate = candidates
        .entry(device.id.clone())
        .or_insert_with(|| DiscoveryCandidate {
            device,
            base_urls: Vec::new(),
        });
    // 避免重复地址被多次加入列表。
    if !candidate.base_urls.contains(&base_url) {
        candidate.base_urls.push(base_url);
    }
    // 按 IPv4 优先的规则重新排序候选地址。
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
    // 先以 mDNS 候选为基础建立按设备 id 索引的表。
    let mut merged = mdns_candidates
        .into_iter()
        .map(|candidate| (candidate.device.id.clone(), candidate))
        .collect::<HashMap<_, _>>();

    // 遍历 UDP 候选，与已有的 mDNS 候选按 id 合并。
    for udp_candidate in udp_candidates {
        merged
            .entry(udp_candidate.device.id.clone())
            .and_modify(|existing| {
                // 已存在同 id 设备：把 UDP 候选中尚未出现过的 base_url 补充进去。
                for base_url in &udp_candidate.base_urls {
                    if !existing.base_urls.contains(base_url) {
                        existing.base_urls.push(base_url.clone());
                    }
                }
                // 合并后重新按优先级排序。
                existing
                    .base_urls
                    .sort_by_key(|url| candidate_url_priority(url));
            })
            // 不存在则直接把 UDP 候选整体插入。
            .or_insert(udp_candidate);
    }

    // 按设备名排序，返回结果顺序稳定。
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
    // 本轮实际发现到的设备 id 集合。
    let discovered_ids = discovered
        .iter()
        .map(|device| device.id.clone())
        .collect::<HashSet<_>>();
    // 本轮已重新发现的设备，清除其历史缺席计数。
    missed_scans.retain(|id, _| !discovered_ids.contains(id));

    // 遍历上一轮的设备列表，找出本轮没有出现的设备。
    for device in previous {
        if discovered_ids.contains(&device.id) {
            continue;
        }
        // 缺席计数加一。
        let misses = missed_scans.entry(device.id.clone()).or_default();
        *misses = misses.saturating_add(1);
        // 未达到移除阈值前，仍把设备保留在结果里（去抖，避免闪烁）。
        if *misses < DISCOVERY_MISSES_BEFORE_REMOVAL {
            discovered.push(device.clone());
        }
    }
    // 按名称排序，保证输出顺序稳定。
    discovered.sort_by(|left, right| left.name.cmp(&right.name));
    discovered
}

// 应用核心服务：持有 HTTP 客户端、数据存储路径与内存态、在线设备快照、
// 发现去抖计数、Hook 配置写入互斥锁、Hook 中继状态等所有共享状态。
// Clone 只克隆 Arc/Client 句柄，底层状态仍然共享。
#[derive(Clone)]
pub struct MonitorService {
    // 复用的 reqwest HTTP 客户端。
    client: Client,
    // 本地持久化数据文件路径。
    data_path: PathBuf,
    // 默认探测到的各 AI 工具 Hook 配置目录。
    default_hook_config_directories: HookConfigDirectories,
    // 内存中的已保存监控数据（读写锁保护）。
    data: Arc<RwLock<SavedMonitorData>>,
    // 当前在线设备快照（后台轮询线程更新）。
    online_devices: Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
    // 各设备连续未被发现命中的次数，用于去抖判定离线。
    discovery_missed_scans: Arc<Mutex<HashMap<String, u8>>>,
    // Hook 配置文件写入互斥锁，避免并发写入导致文件损坏。
    hook_config_write_lock: Arc<Mutex<()>>,
    // Hook 中继监听/转发状态（供前端查询展示）。
    relay_status: Arc<RwLock<HookRelayStatus>>,
}

// Hook 中继状态快照，序列化后暴露给前端展示中继运行情况。
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRelayStatus {
    // 中继 TCP 监听器是否正在运行。
    pub listening: bool,
    // 监听绑定的地址。
    pub bind_address: String,
    // 监听端口。
    pub port: u16,
    // 累计收到的 Hook 请求数。
    pub received_count: u64,
    // 累计成功转发的次数。
    pub forwarded_count: u64,
    // 累计转发失败的次数。
    pub failed_count: u64,
    // 累计被抑制（忽略）未转发的次数。
    pub suppressed_count: u64,
    // 当前排队等待处理的数量。
    pub pending_count: u64,
    // 最近一次处理涉及的 AI 工具。
    pub last_tool: Option<AiTool>,
    // 最近一次收到的 Hook 类型（原始事件名）。
    pub last_hook_type: String,
    // 最近一次转换出的行为类型。
    pub last_behavior: Option<HookBehavior>,
    // 最近一次的错误信息（无错误时为空字符串）。
    pub last_error: String,
}

// Hook 请求体保留工具原生的生命周期上下文。中继当前只读取事件名，但允许
// session_id、turn_id、reason 等原始字段随请求进入，状态算法可按需继续扩展。
#[derive(Deserialize)]
struct HookRequest {
    #[serde(default, rename = "type")]
    legacy_type: Option<String>,
    #[serde(default)]
    hook_event_name: Option<String>,
    #[serde(default, alias = "conversation_id")]
    session_id: Option<String>,
    #[serde(default, alias = "generation_id")]
    turn_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct IncomingHookEvent {
    tool: AiTool,
    hook_type: String,
    session_id: Option<String>,
    turn_id: Option<String>,
    status: Option<String>,
}

// 向设备端上报槽位状态更新时使用的请求体。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SlotUpdateRequest<'a> {
    username: &'a str,
    ai_name: &'static str,
    behavior: HookBehavior,
    content: &'a str,
    image: &'a str,
}

// 连接测试结果，返回给前端展示设备是否可达。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub reachable: bool,
    pub base_url: String,
    pub message: String,
}

// 从设备下载的远程图片，image 字段为 base64 编码内容。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteImage {
    pub filename: String,
    pub mime_type: String,
    pub image: String,
}

// 设备端图片列表接口的响应体。
#[derive(Deserialize)]
struct ImageListResponse {
    images: Vec<RemoteImageMetadata>,
}

// 图片列表中单条图片的元数据（文件名与 MIME 类型）。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteImageMetadata {
    filename: String,
    mime_type: String,
}

// 图片上传接口成功后返回的响应体，携带最终保存的文件名。
#[derive(Deserialize)]
struct UploadResponse {
    filename: String,
}

// 从前端接收的待上传图片：文件名、MIME 类型与原始字节内容。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageUpload {
    filename: String,
    mime_type: String,
    bytes: Vec<u8>,
}

// 设备端错误响应体的通用结构，仅包含一条错误信息字符串。
#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
}

impl MonitorService {
    // 加载/初始化服务：确保配置目录存在，读取（或创建默认的）本地持久化数据，
    // 校验数据合法性后构造出内存态的各共享状态。
    pub fn load(app_data_dir: &Path, config_home: &Path) -> Result<Self, String> {
        // 应用数据目录不存在则递归创建。
        fs::create_dir_all(app_data_dir).map_err(|error| format!("无法创建配置目录：{error}"))?;
        let data_path = app_data_dir.join(STORE_FILENAME);
        let mut data = if data_path.exists() {
            // 存储文件已存在：读取内容并反序列化为 SavedMonitorData。
            let contents =
                fs::read_to_string(&data_path).map_err(|error| format!("无法读取配置：{error}"))?;
            serde_json::from_str(&contents).map_err(|error| format!("配置文件格式错误：{error}"))?
        } else {
            // 首次启动：使用默认空数据。
            SavedMonitorData::default()
        };
        if data.settings.username.trim().is_empty()
            && let Some(username) = detect_system_username(config_home)
        {
            data.settings.username = username;
        }
        // 无论是读取到的还是默认数据，都要过一遍领域层校验，防止带着非法数据启动。
        validate_saved_monitor_data(&data).map_err(|error| format!("配置数据校验失败：{error}"))?;
        Ok(Self {
            client: Client::new(),
            data_path,
            default_hook_config_directories: detect_hook_config_directories(config_home),
            data: Arc::new(RwLock::new(data)),
            online_devices: Arc::new(RwLock::new(Vec::new())),
            discovery_missed_scans: Arc::new(Mutex::new(HashMap::new())),
            hook_config_write_lock: Arc::new(Mutex::new(())),
            // 中继状态初始值：尚未开始监听，绑定地址/端口先填好，其余字段用 Default。
            relay_status: Arc::new(RwLock::new(HookRelayStatus {
                bind_address: HOOK_BIND_ADDRESS.to_owned(),
                port: HOOK_LISTENER_PORT,
                ..HookRelayStatus::default()
            })),
        })
    }

    // 启动本机 Hook 中继：开一个 TCP 监听线程接收本地 Hook 请求，
    // 再开一个工作线程从通道里取出请求做去抖判断并转发给设备。
    pub fn start_hook_listener(&self) {
        let data = Arc::clone(&self.data);
        let online_devices = Arc::clone(&self.online_devices);
        let status = Arc::clone(&self.relay_status);
        thread::spawn(move || {
            // 绑定本地监听端口，失败则记录错误并直接结束该线程（不重试）。
            let listener = match TcpListener::bind((HOOK_BIND_ADDRESS, HOOK_LISTENER_PORT)) {
                Ok(listener) => listener,
                Err(error) => {
                    record_relay_failure(&status, format!("Hook 监听端口启动失败：{error}"));
                    return;
                }
            };
            // 绑定成功后更新状态为“正在监听”，并清空历史错误信息。
            if let Ok(mut current) = status.write() {
                current.listening = true;
                current.last_error.clear();
            }
            // 构造用于向设备转发请求的阻塞式 HTTP 客户端，连接和读写超时都设为 2 秒。
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
            // 建立一个有界 mpsc 通道：listener 只负责接收、解析并交给状态机 worker。
            // 状态推进与设备网络投递已拆成两个阶段，worker 不会被慢设备阻塞；容量
            // 仍设为固定值，为异常洪峰提供背压并从根源上避免原始事件无界堆积。
            let (sender, receiver) =
                mpsc::sync_channel::<IncomingHookEvent>(HOOK_EVENT_QUEUE_CAPACITY);
            let worker_data = Arc::clone(&data);
            let worker_status = Arc::clone(&status);
            spawn_hook_worker(
                &client,
                receiver,
                &worker_data,
                &online_devices,
                worker_status,
            );

            // 主循环：逐个接受 TCP 连接（阻塞式监听器，来一个处理一个）。
            for connection in listener.incoming() {
                let Ok(mut stream) = connection else {
                    continue;
                };
                match read_hook_request(&mut stream) {
                    Ok(event) => {
                        // 解析成功先把待处理计数加一。
                        if let Ok(mut current) = status.write() {
                            current.pending_count += 1;
                        }
                        if sender.send(event).is_ok() {
                            // 成功入队后立刻回 202，表示已接受、异步处理。
                            write_http_response(&mut stream, 202, "Accepted");
                        } else {
                            // 工作线程已经退出导致发送失败：回滚待处理计数，返回 503，并记录失败。
                            if let Ok(mut current) = status.write() {
                                current.pending_count = current.pending_count.saturating_sub(1);
                            }
                            write_http_response(&mut stream, 503, "Service Unavailable");
                            record_relay_failure(&status, "Hook 转发工作线程已停止".to_owned());
                        }
                    }
                    Err(error) => {
                        // 请求解析失败：返回 400 并记录错误信息。
                        write_http_response(&mut stream, 400, "Bad Request");
                        record_relay_failure(&status, error);
                    }
                }
            }
            // 监听循环退出（理论上不会正常发生）后，把状态标记为未监听。
            if let Ok(mut current) = status.write() {
                current.listening = false;
            }
        });
    }

    // 读取当前 Hook 中继状态快照，供前端查询展示。
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
        // clone 出的 service 只是共享状态的句柄，可以安全移动进新线程。
        let service = self.clone();
        thread::spawn(move || {
            let mut next_run = Instant::now();
            loop {
                // 到达计划执行时间才真正跑一次发现，否则只是短暂休眠等待。
                if Instant::now() >= next_run {
                    // 先做设备发现（mDNS+UDP），再对候选逐个做连接测试确定最终在线列表。
                    let result = Self::discover_device_candidates().and_then(|candidates| {
                        tauri::async_runtime::block_on(service.finish_device_discovery(candidates))
                    });
                    if let Ok(devices) = result {
                        // 发布最新在线设备快照，变化时会向前端发事件。
                        let _ = service.publish_online_devices(&app, devices);
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

    // 保存用户在设置页配置的自动发现检查间隔（分钟），校验后持久化并返回最新设置。
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

    /// 保存设置页勾选的 AI 客户端，按固定顺序去重后供两个管理页面共同使用。
    pub fn save_enabled_ai_tools(&self, tools: &[AiTool]) -> Result<MonitorSettings, String> {
        let tools = normalize_enabled_ai_tools(tools);
        let mut data = self
            .data
            .write()
            .map_err(|_| "配置写入锁已损坏".to_owned())?;
        let mut next_data = data.clone();
        next_data.settings.enabled_ai_tools = tools;
        self.persist(&next_data)?;
        *data = next_data;
        Ok(data.settings.clone())
    }

    // 将一批设备发现结果发布为最新在线快照：先做去抖稳定处理，
    // 再在必要时自动选中第一台可用设备，最后仅在快照真正变化时才广播事件。
    pub fn publish_online_devices(
        &self,
        app: &AppHandle,
        devices: Vec<DiscoveredMonitorDevice>,
    ) -> Result<Vec<DiscoveredMonitorDevice>, String> {
        let devices = if let Ok(mut missed_scans) = self.discovery_missed_scans.lock() {
            // 取出上一轮在线快照作为对比基准。
            let previous = self
                .online_devices
                .read()
                .map_or_else(|_| Vec::new(), |current| current.clone());
            // 结合缺席计数做去抖，得到本轮稳定后的设备列表。
            stabilize_discovered_devices(&previous, devices, &mut missed_scans)
        } else {
            devices
        };
        // 若当前选中设备已不在线，自动切换到列表中第一台可用设备。
        self.select_first_available_device_if_needed(&devices)?;
        // 只有快照真正发生变化时才替换并向前端广播事件，避免无意义重绘。
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
        // 没有任何在线设备则无需处理。
        let Some(next) = devices.first() else {
            return Ok(false);
        };
        let settings = self.settings()?;
        // 当前配置的设备 id 仍在在线列表中，无需切换。
        if devices.iter().any(|device| device.id == settings.device_id) {
            return Ok(false);
        }
        // 否则切换到列表中的第一台设备。
        self.select_device(next)?;
        Ok(true)
    }

    // 用新的在线设备列表替换内存快照；内容完全相同则不替换，返回是否发生了替换。
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

    // 读取当前持久化的设置数据（克隆一份返回，避免长期持有锁）。
    pub fn settings(&self) -> Result<MonitorSettings, String> {
        self.data
            .read()
            .map(|data| data.settings.clone())
            .map_err(|_| "配置读取锁已损坏".to_owned())
    }

    // 选中某台设备作为当前使用的监控设备：校验路由信息合法后写入设置，
    // 同时把该设备的路由信息记录/更新进历史设备列表并按 id 排序。
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
        // 更新当前生效的 base_url / device_id / device_name。
        next_data.settings.base_url.clone_from(&route.base_url);
        next_data.settings.device_id.clone_from(&route.device_id);
        next_data
            .settings
            .device_name
            .clone_from(&route.device_name);
        // 从历史设备列表中移除同 id 的旧记录，再插入本次最新的路由信息。
        next_data
            .devices
            .retain(|existing| existing.device_id != route.device_id);
        next_data.devices.push(route);
        // 按设备 id 排序，保持列表顺序稳定。
        next_data
            .devices
            .sort_by(|left, right| left.device_id.cmp(&right.device_id));
        self.persist(&next_data)?;
        *data = next_data;
        Ok(data.settings.clone())
    }

    // 保存用户名：先做领域层校验，再持久化写入设置。
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
        // 用 thread::scope 并发跑 mDNS 发现和 UDP 广播发现两路，互不阻塞。
        let (mdns_result, udp_result) = thread::scope(|scope| {
            let mdns = scope.spawn(Self::discover_mdns_candidates);
            let udp = scope.spawn(discover_udp_candidates);
            (
                // 子线程 panic 时也转换成 Err，不让整个发现流程 panic。
                mdns.join()
                    .unwrap_or_else(|_| Err("mDNS 发现线程异常退出".to_owned())),
                udp.join()
                    .unwrap_or_else(|_| Err("UDP 发现线程异常退出".to_owned())),
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
            (Err(mdns_error), Err(udp_error)) => Err(format!(
                "mDNS 发现失败：{mdns_error}；UDP 广播发现失败：{udp_error}"
            )),
        }
    }

    // 通过 mDNS 浏览 `_aimonitor._tcp.local.` 服务类型，在超时时间内收集所有被解析出的设备。
    fn discover_mdns_candidates() -> Result<Vec<DiscoveryCandidate>, String> {
        // 创建 mDNS 守护实例。
        let daemon = ServiceDaemon::new().map_err(|error| format!("无法启动设备发现：{error}"))?;
        // 缩短网卡状态刷新周期到 1 秒，尽量及时感知网络变化（如切换 Wi-Fi）。
        daemon
            .set_ip_check_interval(1)
            .map_err(|error| format!("无法启用网卡刷新：{error}"))?;
        // 开始浏览指定服务类型，得到一个事件接收器。
        let receiver = daemon
            .browse(AIMONITOR_SERVICE_TYPE)
            .map_err(|error| format!("无法扫描 AIMonitor 设备：{error}"))?;
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
    ) -> Result<Vec<DiscoveredMonitorDevice>, String> {
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

    // 检测某个 base_url（未指定时用当前保存设置）的设备是否可连接，返回带提示信息的连接状态。
    pub async fn check_connection(
        &self,
        base_url: Option<&str>,
    ) -> Result<ConnectionStatus, String> {
        let base_url = match base_url {
            // 显式传入 url 时先做归一化校验。
            Some(value) => normalize_base_url(value)?,
            // 未传入则使用当前保存的设备地址。
            None => self.settings()?.base_url,
        };
        // 请求设备的 /health 接口，超时时间放宽到 5 秒（比发现探测的超时更长，因为是用户主动触发的检测）。
        let result = self
            .client
            .get(format!("{base_url}/health"))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;

        Ok(match result {
            // HTTP 状态码成功：视为可达。
            Ok(response) if response.status().is_success() => ConnectionStatus {
                reachable: true,
                base_url,
                message: "设备连接正常".to_owned(),
            },
            // 有响应但状态码非成功：视为不可达，附带状态码。
            Ok(response) => ConnectionStatus {
                reachable: false,
                base_url,
                message: format!("设备返回 HTTP {}", response.status().as_u16()),
            },
            // 请求本身失败（网络错误等）：视为不可达，附带错误详情。
            Err(error) => ConnectionStatus {
                reachable: false,
                base_url,
                message: format!("无法连接设备：{error}"),
            },
        })
    }

    // 依次探测一组候选地址，返回第一个可达的地址（找不到则 None）。
    async fn first_reachable_url(&self, base_urls: &[String]) -> Option<String> {
        for base_url in base_urls {
            if self.is_reachable(base_url).await {
                return Some(base_url.clone());
            }
        }
        None
    }

    // 探测单个 base_url 的 /health 接口是否可达（使用更短的探测超时，适合批量扫描）。
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

    // 获取设备端保存的所有图片：先拉取图片列表元数据，再逐张下载图片内容。
    pub async fn images(&self) -> Result<Vec<RemoteImage>, String> {
        let base_url = self.settings()?.base_url;
        let response = self
            .client
            .get(format!("{base_url}/api/images"))
            .send()
            .await
            .map_err(|error| format!("无法读取远端图片：{error}"))?;
        // 校验 HTTP 状态码，非成功状态转换为业务错误。
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

    // 下载单张远端图片并转换为 base64 data url，同时做 MIME 类型和大小校验。
    async fn remote_image(
        &self,
        base_url: &str,
        metadata: RemoteImageMetadata,
    ) -> Result<RemoteImage, String> {
        let filename = metadata.filename.trim();
        // 拼出图片下载 url（内部会对文件名做安全校验，防止路径穿越）。
        let url = remote_image_url(base_url, filename)?;
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| format!("{filename} 读取失败：{error}"))?;
        let response = ensure_success(response).await?;
        // 若响应头带有 Content-Length，提前校验大小是否超限，避免读取超大响应体。
        if let Some(length) = response.content_length() {
            ensure_image_size(length, filename)?;
        }

        // 优先信任响应头里的 Content-Type（去掉可能的 charset 等参数），
        // 找不到或不支持时回退使用元数据里记录的 MIME 类型。
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

        // 读取响应体全部字节。
        let bytes = response
            .bytes()
            .await
            .map_err(|error| format!("{filename} 读取失败：{error}"))?;
        // 读取到实际字节后再次校验大小（防止 Content-Length 缺失或不准确的情况）。
        ensure_image_size(bytes.len() as u64, filename)?;

        // 编码为 base64 data url，方便前端直接用作 img src。
        let image = format!("data:{mime_type};base64,{}", encode_base64(&bytes));
        Ok(RemoteImage {
            filename: filename.to_owned(),
            mime_type,
            image,
        })
    }

    // 批量上传图片到设备：先做业务校验，再逐张压缩后以 multipart 表单上传。
    pub async fn upload_images(&self, images: Vec<ImageUpload>) -> Result<Vec<String>, String> {
        validate_image_uploads(&images)?;
        let base_url = self.settings()?.base_url;
        let mut uploaded = Vec::with_capacity(images.len());

        for image in images {
            let source_filename = image.filename.clone();
            // 上传前在 Rust domain 层完成格式校验、缩放及兼容格式转换。
            let processed = process_image_upload(&source_filename, &image.bytes, &image.mime_type)
                .map_err(|error| format!("{source_filename} 处理失败：{error}"))?;
            ensure_image_size(processed.bytes.len() as u64, &processed.filename)?;
            // 构造 multipart 表单的文件分片。
            let file_part = multipart::Part::bytes(processed.bytes)
                .file_name(processed.filename.clone())
                .mime_str(processed.mime_type)
                .map_err(|error| format!("{source_filename} 的图片类型无效：{error}"))?;
            let response = self
                .client
                .post(format!("{base_url}/api/images"))
                .multipart(multipart::Form::new().part("file", file_part))
                .send()
                .await
                .map_err(|error| format!("{source_filename} 上传失败：{error}"))?;
            let response = ensure_success(response).await?;
            let uploaded_filename = response
                .json::<UploadResponse>()
                .await
                .map(|body| body.filename)
                .map_err(|error| format!("{source_filename} 的上传响应格式错误：{error}"))?;
            uploaded.push(uploaded_filename);
        }

        Ok(uploaded)
    }

    // 删除设备端的一张图片。
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

    // 返回当前选中设备对应的所有 AI Profile（按 device_id 过滤）。
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

    // 返回所有支持的 AI 工具各自的 Hook 配置文件位置信息。
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

    // 保存用户自定义的 Hook 配置目录（可为空字符串表示恢复使用默认目录）。
    pub fn save_hook_config_directory(
        &self,
        tool: AiTool,
        directory: &str,
    ) -> Result<HookConfigLocation, String> {
        let directory = directory.trim();
        if !directory.is_empty() {
            let path = Path::new(directory);
            // 非空目录必须是绝对路径，防止相对路径产生歧义。
            if !path.is_absolute() {
                return Err("Hooks 配置目录必须使用绝对路径".to_owned());
            }
            // 若路径已存在，必须是文件夹而非普通文件。
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

    // 保存某个 AI 工具的展示 Profile：强制绑定当前选中设备的 device_id，
    // 校验通过后按 (device_id, tool) 去重替换，再按 (device_id, slot) 排序持久化。
    pub fn save_profile(&self, profile: AiProfile) -> Result<AiProfile, String> {
        let mut data = self
            .data
            .write()
            .map_err(|_| "AI 配置写入锁已损坏".to_owned())?;
        // 未选择设备时不允许保存 Profile。
        if data.settings.device_id.is_empty() {
            return Err("请先选择 AIMonitor 设备".to_owned());
        }
        let mut profile = profile;
        // 强制使用当前设备 id，忽略调用方可能传入的其他值。
        profile.device_id.clone_from(&data.settings.device_id);
        let profile = validate_profile(profile)?;
        let mut next_data = data.clone();
        // 移除同一设备同一工具的旧 Profile，保证每个 (device_id, tool) 只有一条记录。
        next_data.profiles.retain(|existing| {
            existing.device_id != profile.device_id || existing.tool != profile.tool
        });
        next_data.profiles.push(profile.clone());
        // 按设备再按槽位排序，保证列表顺序稳定。
        next_data.profiles.sort_by(|left, right| {
            left.device_id
                .cmp(&right.device_id)
                .then(left.slot.cmp(&right.slot))
        });
        self.persist(&next_data)?;
        *data = next_data;
        Ok(profile)
    }

    // 将某个 AI 工具的 Hook 配置写入其配置文件：生成新内容、与已有文件合并，
    // 仅在内容真正变化时才落盘写入，并告知调用方该工具是否需要人工复核/重启。
    pub fn write_hook_config(&self, tool: AiTool) -> Result<HookConfigWriteResult, String> {
        // 用互斥锁串行化配置文件写入，避免并发写入互相覆盖或撕裂文件内容。
        let _write_guard = self
            .hook_config_write_lock
            .lock()
            .map_err(|_| "Hooks 配置写入锁已损坏".to_owned())?;
        let data = self
            .data
            .read()
            .map_err(|_| "Hooks 配置读取锁已损坏".to_owned())?;
        // Hook 只连接固定的本机中继，不依赖设备 Profile，可在展示配置之前写入。
        let generated = generate_hook_config(tool)?;
        let location = self.hook_config_location(&data, tool);
        let config_path = PathBuf::from(&location.config_path);
        let mut generated_files = vec![(config_path.clone(), generated)];
        generated_files.extend(
            generate_hook_auxiliary_configs(tool)
                .into_iter()
                .map(|preview| {
                    (
                        Path::new(&location.directory).join(&preview.filename),
                        preview,
                    )
                }),
        );

        // 所有目标先读取并完成冲突/格式校验，再统一写入；任一文件不安全时不留下半套配置。
        let mut writes = Vec::with_capacity(generated_files.len());
        for (path, generated) in generated_files {
            let existing = read_optional_config(&path)?;
            let merged = merge_hook_config(existing.as_deref(), &generated, tool)?;
            let changed = existing.as_deref() != Some(merged.content.as_str());
            writes.push((path, merged, changed));
        }
        let config_changed = writes.iter().any(|(_, _, changed)| *changed);
        for (path, merged, changed) in &writes {
            if *changed {
                write_config(path, &merged.content)?;
            }
        }
        Ok(HookConfigWriteResult {
            // 是否需要审核/重启由各工具的 HookProtocol 实现声明，避免在此处按工具硬编码特判。
            requires_review: hook_requires_review(tool) && config_changed,
            restart_required: hook_restart_required(tool) && config_changed,
            tool,
            filename: config_path.to_string_lossy().into_owned(),
            config_changed,
        })
    }

    // 计算某个工具的 Hook 配置目录与文件路径：优先用户自定义目录，否则使用探测到的默认目录。
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

    // 将内存中的数据序列化为格式化 JSON 并原子写入磁盘存储文件。
    fn persist(&self, data: &SavedMonitorData) -> Result<(), String> {
        let serialized = serde_json::to_string_pretty(data)
            .map_err(|error| format!("无法序列化配置：{error}"))?;
        write_atomic_file(&self.data_path, &serialized, "应用配置")
    }
}

// 状态机已经按接收顺序算出的目标状态。`counts_as_hook` 区分真实入队事件与
// 会话超时产生的内部转换，避免内部 GC 污染 received/pending 统计。
#[derive(Debug)]
struct PendingHookRelay {
    tool: AiTool,
    hook_type: String,
    transition: HookTransition,
    counts_as_hook: bool,
}

type PendingHookRelays = Arc<Mutex<HashMap<AiTool, PendingHookRelay>>>;
type HookRelayWakeSenders = HashMap<AiTool, mpsc::SyncSender<()>>;

fn spawn_hook_worker(
    client: &reqwest::blocking::Client,
    receiver: mpsc::Receiver<IncomingHookEvent>,
    data: &Arc<RwLock<SavedMonitorData>>,
    online_devices: &Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
    status: Arc<RwLock<HookRelayStatus>>,
) {
    // 网络投递使用 latest-wins mailbox：每个工具至多一个正在发送的状态和一个
    // 尚未发送的最新状态。旧的待发送中间态会被覆盖，但所有原始事件仍先按序推进
    // 状态机，因此 Stop/SessionEnd 等时序屏障不会被跳过。
    let pending_relays = Arc::new(Mutex::new(HashMap::<AiTool, PendingHookRelay>::new()));
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
            let pending = pending_relays
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&tool));
            let Some(pending) = pending else {
                continue;
            };
            relay_hook_with_accounting(&client, &data, &online_devices, &status, &pending);
        }
    });
}

fn enqueue_latest_hook_relay(
    pending_relays: &PendingHookRelays,
    wake_senders: &HookRelayWakeSenders,
    status: &Arc<RwLock<HookRelayStatus>>,
    relay: PendingHookRelay,
) {
    let tool = relay.tool;
    let (should_wake, displaced) = if let Ok(mut pending) = pending_relays.lock() {
        let displaced = pending.insert(tool, relay);
        (displaced.is_none(), displaced)
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
            .and_then(|mut pending| pending.remove(&tool));
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
            .and_then(|mut pending| pending.remove(&tool));
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

// 从 Hook 中继监听收到的 TCP 连接里读取并解析出一个 Hook 请求，返回涉及的 AI 工具与事件类型。
fn read_hook_request(stream: &mut TcpStream) -> Result<IncomingHookEvent, String> {
    // 读超时 3 秒，避免恶意或异常连接长期占用线程。
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| format!("无法设置 Hook 请求超时：{error}"))?;
    let mut request = Vec::with_capacity(1024);
    let mut buffer = [0_u8; 1024];
    // 循环读取数据直到找到 HTTP 头结束标志 \r\n\r\n，记录头部结束位置的下标。
    let header_end = loop {
        // 每次读取一段数据追加进缓冲区。
        let read = stream
            .read(&mut buffer)
            .map_err(|error| format!("无法读取 Hook 请求：{error}"))?;
        if read == 0 {
            return Err("Hook 请求未完整发送".to_owned());
        }
        request.extend_from_slice(&buffer[..read]);
        // 超过单次请求最大字节数限制则直接拒绝，防止恶意/异常连接耗尽内存。
        if request.len() > MAX_HOOK_REQUEST_BYTES {
            return Err("Hook 请求过大".to_owned());
        }
        // 找到头部结束标志则跳出循环，记录结束位置（包含标志本身的 4 字节）。
        if let Some(index) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break index + 4;
        }
    };

    // 头部部分必须是合法 UTF-8 文本。
    let headers = std::str::from_utf8(&request[..header_end])
        .map_err(|_| "Hook 请求头不是有效 UTF-8".to_owned())?;
    let mut lines = headers.split("\r\n");
    // 第一行是请求行，例如 "POST /api/hooks/codex HTTP/1.1"。
    let request_line = lines
        .next()
        .ok_or_else(|| "Hook 请求缺少请求行".to_owned())?;
    let mut request_parts = request_line.split_whitespace();
    // 只接受 POST 方法。
    if request_parts.next() != Some("POST") {
        return Err("Hook 接口只接受 POST".to_owned());
    }
    let path = request_parts
        .next()
        .ok_or_else(|| "Hook 请求缺少路径".to_owned())?;
    // 根据路径后缀确定是哪个 AI 工具发来的 Hook 请求；slug 与工具的映射统一由
    // 各 HookProtocol 的 slug() 提供，此处不再重复维护一份镜像表。
    let tool = path
        .strip_prefix("/api/hooks/")
        .and_then(tool_from_slug)
        .ok_or_else(|| "Hook 请求中的 AI 工具无效".to_owned())?;
    // 同时读取正文长度与配置生成时写入的可信事件头。原始 Hook JSON 中通常
    // 自带 hook_event_name；事件头用于 Cursor 等协议字段不一致时兜底。
    let mut content_length = None;
    let mut header_hook_type = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse::<usize>().ok();
        } else if name.eq_ignore_ascii_case("x-aimonitor-hook-type") {
            header_hook_type = Some(value.trim().to_owned());
        }
    }
    let content_length =
        content_length.ok_or_else(|| "Hook 请求缺少有效的 Content-Length".to_owned())?;
    // 请求体长度必须大于 0 且不超过限制。
    if content_length == 0 || content_length > MAX_HOOK_BODY_BYTES {
        return Err("Hook 请求体大小无效".to_owned());
    }

    // 继续读取直到已缓冲的数据长度覆盖了完整的请求体。
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
    // 截取请求体部分并反序列化。未知字段是工具提供的原始上下文，不属于监控
    // 业务载荷，因此予以保留兼容而不是拒绝整个事件。
    let body =
        serde_json::from_slice::<HookRequest>(&request[header_end..header_end + content_length])
            .map_err(|error| format!("Hook 请求 JSON 无效：{error}"))?;
    let body_hook_type = body.hook_event_name.clone().or(body.legacy_type.clone());
    if let (Some(body_type), Some(header_type)) = (&body_hook_type, &header_hook_type)
        && body_type.trim() != header_type.trim()
    {
        return Err("Hook 请求体与事件头不一致".to_owned());
    }
    let hook_type = body_hook_type
        .or(header_hook_type)
        .ok_or_else(|| "Hook 请求缺少事件类型".to_owned())?;
    let hook_type = hook_type.trim();
    // 事件类型字符串不能为空，也不能过长。
    if hook_type.is_empty() || hook_type.len() > 128 {
        return Err("Hook 类型不能为空且不能超过 128 个字符".to_owned());
    }
    let session_id =
        normalize_hook_context_field(body.session_id, "session_id", MAX_HOOK_SESSION_ID_BYTES)?;
    let turn_id = normalize_hook_context_field(body.turn_id, "turn_id", MAX_HOOK_TURN_ID_BYTES)?;
    let status = normalize_hook_context_field(body.status, "status", MAX_HOOK_STATUS_BYTES)?;
    Ok(IncomingHookEvent {
        tool,
        hook_type: hook_type.to_owned(),
        session_id,
        turn_id,
        status,
    })
}

fn normalize_hook_context_field(
    value: Option<String>,
    field: &str,
    max_bytes: usize,
) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > max_bytes {
        return Err(format!("Hook {field} 不能超过 {max_bytes} 个 UTF-8 字节"));
    }
    Ok(Some(value.to_owned()))
}

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
    let online_ids = online_snapshot
        .iter()
        .map(|device| device.id.as_str())
        .collect::<HashSet<_>>();
    // 找出所有已配置该 AI 工具展示 Profile 的历史设备记录（与 Profile 配对）。
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
    // 让在线设备排在前面（不影响后续是否转发，只影响记录里的顺序倾向）。
    targets.sort_by_key(|(device, _)| !online_ids.contains(device.device_id.as_str()));
    // 没有任何目标设备配置了该工具，记录提示信息并返回。
    if targets.is_empty() {
        record_hook_results_with_accounting(
            status,
            tool,
            hook_type,
            None,
            0,
            &["尚未配置该 AI 的转发位置".to_owned()],
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
                // 若该设备当前在线，优先使用在线快照中的最新地址（可能比保存的地址更准确、更及时）；
                // 否则回退使用保存的历史路由信息尝试连接。
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
                // 为每个目标设备单独开一个线程并发转发，互不阻塞。
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

/// 把设备的非 2xx 响应转成对用户有意义的错误：优先使用设备返回的
/// `ErrorResponse.error`，解析不到（包括 204 无正文的情况）时退回到
/// HTTP 状态码文案。供阻塞版和异步版 `ensure_success*` 共用。
fn device_error_message(status: StatusCode, parsed_body: Option<ErrorResponse>) -> String {
    // 优先使用设备返回的错误正文；解析不到（比如 204 无正文）则退回状态码文案。
    parsed_body.map_or_else(
        || format!("设备请求失败（HTTP {}）", status.as_u16()),
        |body| body.error,
    )
}

// 阻塞版的响应成功性校验：非 2xx 时尝试解析错误正文并转换为业务错误。
fn ensure_success_blocking(
    response: reqwest::blocking::Response,
) -> Result<reqwest::blocking::Response, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    // 204 No Content 没有正文可解析，直接跳过解析步骤。
    let parsed_body = if status == StatusCode::NO_CONTENT {
        None
    } else {
        response.json::<ErrorResponse>().ok()
    };
    Err(device_error_message(status, parsed_body))
}

/// 记录一次 Hook 事件已处理完成的公共字段，成功/抑制两条路径在此基础上
/// 各自补充结果专属字段，避免重复维护同一份计数逻辑。
fn begin_hook_completion(current: &mut HookRelayStatus, tool: AiTool, hook_type: &str) {
    // 收到总数加一。
    current.received_count += 1;
    // 待处理计数减一（不会低于 0）。
    current.pending_count = current.pending_count.saturating_sub(1);
    // 记录最近一次涉及的工具和事件类型。
    current.last_tool = Some(tool);
    hook_type.clone_into(&mut current.last_hook_type);
}

// 记录一次真实转发（非抑制）的处理结果：成功次数、失败次数、最近行为与错误信息。
fn record_hook_results(
    status: &Arc<RwLock<HookRelayStatus>>,
    tool: AiTool,
    hook_type: &str,
    behavior: Option<HookBehavior>,
    forwarded: u64,
    errors: &[String],
) {
    record_hook_results_with_accounting(status, tool, hook_type, behavior, forwarded, errors, true);
}

fn record_hook_results_with_accounting(
    status: &Arc<RwLock<HookRelayStatus>>,
    tool: AiTool,
    hook_type: &str,
    behavior: Option<HookBehavior>,
    forwarded: u64,
    errors: &[String],
    counts_as_hook: bool,
) {
    if let Ok(mut current) = status.write() {
        if counts_as_hook {
            begin_hook_completion(&mut current, tool, hook_type);
        } else {
            // 超时清扫产生的是内部状态转换，不凭空增加收到数，也不消耗一个
            // pending；仍更新最近工具/类型，让自动释放在工作台中可解释。
            current.last_tool = Some(tool);
            hook_type.clone_into(&mut current.last_hook_type);
        }
        current.forwarded_count += forwarded;
        current.failed_count += errors.len() as u64;
        current.last_behavior = behavior;
        // 多个设备的错误信息用中文顿号拼接展示。
        current.last_error = errors.join("；");
    }
}

// 记录一次被抑制（未真正转发）的 Hook 事件：仍计入收到总数，但归入抑制计数。
fn record_suppressed_hook(status: &Arc<RwLock<HookRelayStatus>>, tool: AiTool, hook_type: &str) {
    if let Ok(mut current) = status.write() {
        begin_hook_completion(&mut current, tool, hook_type);
        current.suppressed_count += 1;
        // 忽略事件不会改变已经转发到设备的最后行为。
        current.last_error.clear();
    }
}

// 记录一次中继层面的失败（如监听启动失败、请求解析失败等，与具体转发无关）。
fn record_relay_failure(status: &Arc<RwLock<HookRelayStatus>>, error: String) {
    if let Ok(mut current) = status.write() {
        current.failed_count += 1;
        current.last_error = error;
    }
}

// 向 Hook 中继的本地连接写回一个最简单的 HTTP 响应（无正文），随后关闭连接。
fn write_http_response(stream: &mut TcpStream, status: u16, reason: &str) {
    let response =
        format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

// 拼出下载单张图片的完整 URL，同时校验文件名合法，防止路径穿越攻击。
fn remote_image_url(base_url: &str, filename: &str) -> Result<Url, String> {
    // 拒绝空文件名、"." "/.." 以及包含路径分隔符的文件名。
    if filename.is_empty() || filename == "." || filename == ".." || filename.contains(['/', '\\'])
    {
        return Err("远端图片文件名无效".to_owned());
    }

    let mut url = Url::parse(&format!("{base_url}/api/images/"))
        .map_err(|error| format!("设备图片地址无效：{error}"))?;
    // 通过 path_segments_mut 安全地追加文件名段（会自动做 URL 编码），
    // 而不是用字符串拼接，避免特殊字符导致的问题。
    url.path_segments_mut()
        .map_err(|()| "设备图片地址不能包含路径段".to_owned())?
        .pop_if_empty()
        .push(filename);
    Ok(url)
}

// 判断设备端返回的 MIME 类型是否属于其原生支持的图片格式。
fn is_supported_image_mime(mime_type: &str) -> bool {
    matches!(mime_type, "image/jpeg" | "image/png" | "image/gif")
}

// 判断桌面端可以接收并在上传前处理的图片 MIME 类型。
fn is_supported_upload_image_mime(mime_type: &str) -> bool {
    matches!(
        mime_type,
        "image/bmp" | "image/x-ms-bmp" | "image/jpeg" | "image/png" | "image/gif" | "image/webp"
    )
}

// 校验图片字节长度不超过最大限制。
fn ensure_image_size(len: u64, filename: &str) -> Result<(), String> {
    if len > MAX_REMOTE_IMAGE_BYTES as u64 {
        return Err(format!("{filename} 不能超过 8 MiB"));
    }
    Ok(())
}

// 上传前的批量校验：必须至少选择一张图片，且每张图片文件名非空、内容非空、
// 大小不超限、MIME 类型受支持；任意一张不满足都直接整体拒绝（不做部分上传）。
fn validate_image_uploads(images: &[ImageUpload]) -> Result<(), String> {
    if images.is_empty() {
        return Err("请选择要上传的图片".to_owned());
    }

    for image in images {
        if image.filename.trim().is_empty() || image.bytes.is_empty() {
            return Err("所选图片中包含空文件".to_owned());
        }
        ensure_image_size(image.bytes.len() as u64, &image.filename)?;
        if !is_supported_upload_image_mime(&image.mime_type) {
            return Err(format!(
                "{} 不是支持的 BMP、JPEG、GIF、PNG 或 WebP 图片",
                image.filename
            ));
        }
    }

    Ok(())
}

// 读取配置文件内容；文件不存在时返回 Ok(None) 而不是错误（首次写入时很常见）。
fn read_optional_config(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("无法读取 {}：{error}", path.display())),
    }
}

// 写入 Hook 配置文件，复用通用的原子写入逻辑。
fn write_config(path: &Path, content: &str) -> Result<(), String> {
    write_atomic_file(path, content, "Hooks 配置")
}

// 原子写入文件：先写临时文件，再重命名/替换为目标文件，避免写入过程中崩溃导致文件损坏或内容截断。
fn write_atomic_file(path: &Path, content: &str, label: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("无法确定 {} 的配置目录", path.display()))?;
    // 确保目标目录存在。
    fs::create_dir_all(parent)
        .map_err(|error| format!("无法创建配置目录 {}：{error}", parent.display()))?;

    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("配置文件路径无效：{}", path.display()))?;
    // 临时文件名以 . 开头并带特殊后缀，避免与正常文件冲突或被误读。
    let temporary_path = parent.join(format!(".{filename}.aimonitor.tmp"));
    fs::write(&temporary_path, content)
        .map_err(|error| format!("无法写入临时配置 {}：{error}", temporary_path.display()))?;

    // 类 Unix 系统上 rename 是原子操作，可以安全地替换目标文件；
    // Windows 上 rename 到已存在文件会失败，因此改用直接写入+删除临时文件的方式。
    #[cfg(not(windows))]
    let replace_result = fs::rename(&temporary_path, path);
    #[cfg(windows)]
    let replace_result = fs::write(path, content).and_then(|()| fs::remove_file(&temporary_path));

    if let Err(error) = replace_result {
        // 替换失败时尽力清理临时文件，避免遗留垃圾文件。
        let _ = fs::remove_file(&temporary_path);
        return Err(format!("无法写入{label} {}：{error}", path.display()));
    }
    Ok(())
}

// 异步版的响应成功性校验（对应阻塞版 ensure_success_blocking），逻辑相同。
async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let parsed_body = if status == StatusCode::NO_CONTENT {
        None
    } else {
        response.json::<ErrorResponse>().await.ok()
    };
    Err(device_error_message(status, parsed_body))
}

#[cfg(test)]
mod tests {
    // 测试专用导入：IP 地址类型、系统时间。
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::time::{SystemTime, UNIX_EPOCH};

    // mDNS 测试辅助类型：接口标识、作用域 IPv4 地址。
    use mdns_sd::{InterfaceId, ScopedIpV4};

    use crate::domain::monitor::{HookBehavior, HookContent};

    // 引入外层模块（本文件）的全部公共/私有项，便于直接测试内部函数。
    use super::*;

    // 构造一个测试用的 AI Profile：Codex 工具、槽位 1，四种行为各配一张示例图片。
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

    fn two_tool_delivery_data(
        codex_address: SocketAddr,
        claude_address: SocketAddr,
    ) -> SavedMonitorData {
        let mut claude_profile = test_profile();
        claude_profile.tool = AiTool::ClaudeCode;
        claude_profile.device_id = "screen-2".to_owned();
        claude_profile.slot = 2;
        SavedMonitorData {
            settings: MonitorSettings {
                base_url: format!("http://{codex_address}"),
                username: "Manon".to_owned(),
                device_id: "screen-1".to_owned(),
                device_name: "Desk".to_owned(),
                ..MonitorSettings::default()
            },
            devices: vec![
                MonitorDeviceRoute {
                    base_url: format!("http://{codex_address}"),
                    device_id: "screen-1".to_owned(),
                    device_name: "Desk".to_owned(),
                },
                MonitorDeviceRoute {
                    base_url: format!("http://{claude_address}"),
                    device_id: "screen-2".to_owned(),
                    device_name: "Studio".to_owned(),
                },
            ],
            profiles: vec![test_profile(), claude_profile],
            hook_config_directories: HookConfigDirectories::default(),
        }
    }

    #[test]
    fn empty_username_defaults_to_the_local_system_username() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ai-monitor-default-username-{}-{unique}",
            std::process::id()
        ));
        let app_data = root.join("app-data");
        let config_home = root.join("fallback-user");
        let expected = detect_system_username(&config_home).unwrap();

        let service = MonitorService::load(&app_data, &config_home).unwrap();

        assert_eq!(service.settings().unwrap().username, expected);
        fs::remove_dir_all(root).unwrap();
    }

    // 验证 read_hook_request 能正确解析一个合法的最小 Hook 请求：
    // 只有工具路径和 type 字段的请求体应被成功解析出 (工具, 事件类型)。
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
        // 用另一个线程模拟客户端连接并发送请求。
        let sender = thread::spawn(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            stream.write_all(request.as_bytes()).unwrap();
        });
        let (mut stream, _) = listener.accept().unwrap();

        let request = read_hook_request(&mut stream).unwrap();

        sender.join().unwrap();
        // 期望解析出 Codex 工具与 SessionStart 事件类型。
        assert_eq!(
            request,
            IncomingHookEvent {
                tool: AiTool::Codex,
                hook_type: "SessionStart".to_owned(),
                session_id: None,
                turn_id: None,
                status: None,
            }
        );
    }

    // 验证工具原生 Hook JSON 可以完整透传：状态算法只读取 hook_event_name，
    // session_id/turn_id/tool_input 等上下文不会导致请求被拒绝。
    #[test]
    fn local_hook_request_accepts_native_context_and_event_header() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let body = r#"{"hook_event_name":"stop","conversation_id":"s-1","generation_id":"t-1","status":"cancelled"}"#;
        let request = format!(
            "POST /api/hooks/cursor HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Content-Type: application/json\r\nX-AIMonitor-Hook-Type: stop\r\n\
             Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let sender = thread::spawn(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            stream.write_all(request.as_bytes()).unwrap();
        });
        let (mut stream, _) = listener.accept().unwrap();

        let parsed = read_hook_request(&mut stream).unwrap();

        sender.join().unwrap();
        assert_eq!(
            parsed,
            IncomingHookEvent {
                tool: AiTool::Cursor,
                hook_type: "stop".to_owned(),
                session_id: Some("s-1".to_owned()),
                turn_id: Some("t-1".to_owned()),
                status: Some("cancelled".to_owned()),
            }
        );
    }

    #[test]
    fn local_hook_request_rejects_context_identifiers_that_could_bloat_the_queue() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let oversized_session_id = "s".repeat(MAX_HOOK_SESSION_ID_BYTES + 1);
        let body = format!(
            r#"{{"hook_event_name":"UserPromptSubmit","session_id":"{oversized_session_id}"}}"#
        );
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

        let error = read_hook_request(&mut stream).unwrap_err();

        sender.join().unwrap();
        assert!(error.contains("session_id"));
        assert!(error.contains(&MAX_HOOK_SESSION_ID_BYTES.to_string()));
    }

    #[test]
    fn latest_relay_mailbox_keeps_only_the_newest_state_per_tool() {
        let pending_relays = Arc::new(Mutex::new(HashMap::new()));
        let (wake_sender, wake_receiver) = mpsc::sync_channel::<()>(HOOK_RELAY_WAKE_QUEUE_CAPACITY);
        let wake_senders = HashMap::from([(AiTool::Codex, wake_sender)]);
        let status = Arc::new(RwLock::new(HookRelayStatus {
            pending_count: 2,
            ..HookRelayStatus::default()
        }));

        enqueue_latest_hook_relay(
            &pending_relays,
            &wake_senders,
            &status,
            PendingHookRelay {
                tool: AiTool::Codex,
                hook_type: "UserPromptSubmit".to_owned(),
                transition: HookTransition::Display(HookBehavior::Running),
                counts_as_hook: true,
            },
        );
        enqueue_latest_hook_relay(
            &pending_relays,
            &wake_senders,
            &status,
            PendingHookRelay {
                tool: AiTool::Codex,
                hook_type: "PermissionRequest".to_owned(),
                transition: HookTransition::Display(HookBehavior::Asking),
                counts_as_hook: true,
            },
        );

        assert_eq!(wake_receiver.try_recv(), Ok(()));
        assert_eq!(wake_receiver.try_recv(), Err(mpsc::TryRecvError::Empty));
        let pending = pending_relays.lock().unwrap();
        let latest = pending.get(&AiTool::Codex).unwrap();
        assert_eq!(latest.hook_type, "PermissionRequest");
        assert_eq!(
            latest.transition,
            HookTransition::Display(HookBehavior::Asking)
        );
        let status = status.read().unwrap();
        assert_eq!(status.received_count, 1);
        assert_eq!(status.suppressed_count, 1);
        assert_eq!(status.pending_count, 1);
    }

    #[test]
    fn hook_delivery_workers_are_isolated_per_tool() {
        let codex_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let codex_address = codex_listener.local_addr().unwrap();
        let claude_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let claude_address = claude_listener.local_addr().unwrap();
        let (started_sender, started_receiver) = mpsc::channel::<&'static str>();
        let (release_sender, release_receiver) = mpsc::channel::<()>();

        let codex_started_sender = started_sender.clone();
        let codex_server = thread::spawn(move || {
            let (mut stream, _) = codex_listener.accept().unwrap();
            read_test_http_request(&mut stream);
            codex_started_sender.send("codex").unwrap();
            release_receiver
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });
        let claude_server = thread::spawn(move || {
            let (mut stream, _) = claude_listener.accept().unwrap();
            read_test_http_request(&mut stream);
            started_sender.send("claude").unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });

        let data = Arc::new(RwLock::new(two_tool_delivery_data(
            codex_address,
            claude_address,
        )));
        let online_devices = Arc::new(RwLock::new(Vec::new()));
        let status = Arc::new(RwLock::new(HookRelayStatus {
            pending_count: 2,
            ..HookRelayStatus::default()
        }));
        let pending_relays = Arc::new(Mutex::new(HashMap::new()));
        let wake_senders = spawn_hook_delivery_workers(
            &reqwest::blocking::Client::new(),
            &pending_relays,
            &data,
            &online_devices,
            &status,
        );

        enqueue_latest_hook_relay(
            &pending_relays,
            &wake_senders,
            &status,
            PendingHookRelay {
                tool: AiTool::Codex,
                hook_type: "UserPromptSubmit".to_owned(),
                transition: HookTransition::Display(HookBehavior::Running),
                counts_as_hook: true,
            },
        );
        assert_eq!(
            started_receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
            "codex"
        );

        enqueue_latest_hook_relay(
            &pending_relays,
            &wake_senders,
            &status,
            PendingHookRelay {
                tool: AiTool::ClaudeCode,
                hook_type: "UserPromptSubmit".to_owned(),
                transition: HookTransition::Display(HookBehavior::Running),
                counts_as_hook: true,
            },
        );
        assert_eq!(
            started_receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
            "claude"
        );

        release_sender.send(()).unwrap();
        codex_server.join().unwrap();
        claude_server.join().unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if status.read().unwrap().forwarded_count == 2 {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let status = status.read().unwrap();
        assert_eq!(status.forwarded_count, 2);
        assert!(status.last_error.is_empty());
    }

    #[test]
    fn latest_relay_mailbox_allows_one_in_flight_and_one_newest_pending_state() {
        let pending_relays = Arc::new(Mutex::new(HashMap::new()));
        let (wake_sender, wake_receiver) = mpsc::sync_channel::<()>(HOOK_RELAY_WAKE_QUEUE_CAPACITY);
        let wake_senders = HashMap::from([(AiTool::Codex, wake_sender)]);
        let status = Arc::new(RwLock::new(HookRelayStatus {
            pending_count: 3,
            ..HookRelayStatus::default()
        }));

        enqueue_latest_hook_relay(
            &pending_relays,
            &wake_senders,
            &status,
            PendingHookRelay {
                tool: AiTool::Codex,
                hook_type: "UserPromptSubmit".to_owned(),
                transition: HookTransition::Display(HookBehavior::Running),
                counts_as_hook: true,
            },
        );
        assert_eq!(wake_receiver.try_recv(), Ok(()));
        // 模拟 delivery worker 已取走 Running 并正在等待设备响应。
        let in_flight = pending_relays
            .lock()
            .unwrap()
            .remove(&AiTool::Codex)
            .unwrap();
        assert_eq!(
            in_flight.transition,
            HookTransition::Display(HookBehavior::Running)
        );

        enqueue_latest_hook_relay(
            &pending_relays,
            &wake_senders,
            &status,
            PendingHookRelay {
                tool: AiTool::Codex,
                hook_type: "PermissionRequest".to_owned(),
                transition: HookTransition::Display(HookBehavior::Asking),
                counts_as_hook: true,
            },
        );
        enqueue_latest_hook_relay(
            &pending_relays,
            &wake_senders,
            &status,
            PendingHookRelay {
                tool: AiTool::Codex,
                hook_type: "Stop".to_owned(),
                transition: HookTransition::Display(HookBehavior::Idle),
                counts_as_hook: true,
            },
        );

        assert_eq!(wake_receiver.try_recv(), Ok(()));
        assert_eq!(wake_receiver.try_recv(), Err(mpsc::TryRecvError::Empty));
        let pending = pending_relays.lock().unwrap();
        let latest = pending.get(&AiTool::Codex).unwrap();
        assert_eq!(latest.hook_type, "Stop");
        assert_eq!(
            latest.transition,
            HookTransition::Display(HookBehavior::Idle)
        );
        let status = status.read().unwrap();
        assert_eq!(status.received_count, 1);
        assert_eq!(status.suppressed_count, 1);
        // Running 仍在发送，Idle 仍待发送；被覆盖的 Asking 已完成记账。
        assert_eq!(status.pending_count, 2);
    }

    #[test]
    fn timeout_and_real_hook_mailbox_replacements_keep_hook_metrics_exact() {
        let timeout = || PendingHookRelay {
            tool: AiTool::Codex,
            hook_type: "SessionTimeout".to_owned(),
            transition: HookTransition::Release,
            counts_as_hook: false,
        };
        let real_hook = || PendingHookRelay {
            tool: AiTool::Codex,
            hook_type: "UserPromptSubmit".to_owned(),
            transition: HookTransition::Display(HookBehavior::Running),
            counts_as_hook: true,
        };

        // 内部超时覆盖真实待投递事件时，真实事件必须完成 pending/received 记账。
        let pending_relays = Arc::new(Mutex::new(HashMap::new()));
        let (wake_sender, _wake_receiver) =
            mpsc::sync_channel::<()>(HOOK_RELAY_WAKE_QUEUE_CAPACITY);
        let wake_senders = HashMap::from([(AiTool::Codex, wake_sender)]);
        let status = Arc::new(RwLock::new(HookRelayStatus {
            pending_count: 1,
            ..HookRelayStatus::default()
        }));
        enqueue_latest_hook_relay(&pending_relays, &wake_senders, &status, real_hook());
        enqueue_latest_hook_relay(&pending_relays, &wake_senders, &status, timeout());
        {
            let status = status.read().unwrap();
            assert_eq!(status.received_count, 1);
            assert_eq!(status.suppressed_count, 1);
            assert_eq!(status.pending_count, 0);
        }

        // 真实事件覆盖内部超时时，不应为被覆盖的内部转换虚增任何 Hook 指标。
        let pending_relays = Arc::new(Mutex::new(HashMap::new()));
        let (wake_sender, _wake_receiver) =
            mpsc::sync_channel::<()>(HOOK_RELAY_WAKE_QUEUE_CAPACITY);
        let wake_senders = HashMap::from([(AiTool::Codex, wake_sender)]);
        let status = Arc::new(RwLock::new(HookRelayStatus {
            pending_count: 1,
            ..HookRelayStatus::default()
        }));
        enqueue_latest_hook_relay(&pending_relays, &wake_senders, &status, timeout());
        enqueue_latest_hook_relay(&pending_relays, &wake_senders, &status, real_hook());
        let status = status.read().unwrap();
        assert_eq!(status.received_count, 0);
        assert_eq!(status.suppressed_count, 0);
        assert_eq!(status.pending_count, 1);
        assert_eq!(
            pending_relays.lock().unwrap()[&AiTool::Codex].hook_type,
            "UserPromptSubmit"
        );
    }

    #[test]
    fn synthetic_timeout_delivery_does_not_consume_a_hook_metric() {
        let status = Arc::new(RwLock::new(HookRelayStatus {
            pending_count: 7,
            ..HookRelayStatus::default()
        }));

        record_hook_results_with_accounting(
            &status,
            AiTool::Codex,
            "SessionTimeout",
            None,
            2,
            &[],
            false,
        );

        let status = status.read().unwrap();
        assert_eq!(status.received_count, 0);
        assert_eq!(status.pending_count, 7);
        assert_eq!(status.forwarded_count, 2);
        assert_eq!(status.last_hook_type, "SessionTimeout");
    }

    #[test]
    fn local_hook_request_routes_new_tool_slugs() {
        for (slug, tool) in [
            ("harness", AiTool::Harness),
            ("openclaw", AiTool::OpenClaw),
            ("codebuddy", AiTool::CodeBuddy),
        ] {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let address = listener.local_addr().unwrap();
            let body = r#"{"hook_event_name":"probe"}"#;
            let request = format!(
                "POST /api/hooks/{slug} HTTP/1.1\r\nHost: 127.0.0.1\r\n\
                 Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            let sender = thread::spawn(move || {
                let mut stream = TcpStream::connect(address).unwrap();
                stream.write_all(request.as_bytes()).unwrap();
            });
            let (mut stream, _) = listener.accept().unwrap();
            let parsed = read_hook_request(&mut stream).unwrap();
            sender.join().unwrap();
            assert_eq!(parsed.tool, tool);
            assert_eq!(parsed.hook_type, "probe");
        }
    }

    // 验证 relay_hook 能根据事件类型计算出正确的行为状态，并转发到已配置的设备路由，
    // 同时正确更新中继状态（收到数、转发数、最近行为、无错误信息）。
    #[test]
    fn relay_computes_state_and_uses_a_configured_device_route() {
        // 启动一个本地监听器模拟设备端，接收请求后立即回 200。
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let receiver = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_test_http_request(&mut stream);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
            String::from_utf8(request).unwrap()
        });
        // 构造保存的配置数据：当前设备指向刚才的模拟监听地址，并带上一个 Codex Profile。
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
        // 在线设备列表为空，验证会回退使用保存的设备路由。
        let online_devices = Arc::new(RwLock::new(Vec::new()));

        // 触发一次 UserPromptSubmit 事件（应转换为 Running 行为）。
        relay_hook(
            &reqwest::blocking::Client::new(),
            &data,
            &online_devices,
            &status,
            AiTool::Codex,
            "UserPromptSubmit",
            HookTransition::Display(HookBehavior::Running),
        );

        let request = receiver.join().unwrap();
        // 断言请求路径、用户名、AI 名称、行为、图片内容均正确携带。
        assert!(request.starts_with("POST /api/slots/1 HTTP/1.1"));
        assert!(request.contains(r#""username":"Manon""#));
        assert!(request.contains(r#""aiName":"Codex""#));
        assert!(request.contains(r#""behavior":"running""#));
        assert!(request.contains(r#""image":"running.gif""#));
        let status = status.read().unwrap();
        // 断言中继状态被正确更新：收到 1 次，转发成功 1 次，最近行为为 Running，无错误。
        assert_eq!(status.received_count, 1);
        assert_eq!(status.forwarded_count, 1);
        assert_eq!(status.last_behavior, Some(HookBehavior::Running));
        assert!(status.last_error.is_empty());
    }

    // 验证转发时会优先使用在线快照中的最新地址：设备 screen-1 保存的地址不可达（已 drop 掉监听器），
    // 设备 screen-2 保存的地址同样不可达，但在线快照里提供了它的新地址 available_address；
    // 两台设备各触发一次转发（一次失败一次成功），断言实际收到请求的是使用了在线新地址的那台。
    #[test]
    fn relay_prioritizes_online_routes_and_uses_their_latest_address() {
        // 绑定后立即 drop，得到一个必定连接失败的地址。
        let unavailable_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let unavailable_address = unavailable_listener.local_addr().unwrap();
        drop(unavailable_listener);

        // 真正可用的模拟设备服务器。
        let available_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let available_address = available_listener.local_addr().unwrap();
        let receiver = thread::spawn(move || {
            let (mut stream, _) = available_listener.accept().unwrap();
            let request = read_test_http_request(&mut stream);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
            String::from_utf8(request).unwrap()
        });

        // 第二台设备（screen-2）的 Profile，使用不同槽位号 7 便于断言区分。
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
            // 两台设备的历史保存地址都指向不可达地址。
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
        // 在线快照只包含 screen-2，且地址是真正可用的 available_address。
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
            HookTransition::Display(HookBehavior::Running),
        );

        let request = receiver.join().unwrap();
        // 实际收到的请求应该是 screen-2 的槽位 7（证明使用了在线地址而非保存的不可达地址）。
        assert!(request.starts_with("POST /api/slots/7 HTTP/1.1"));
        assert!(request.contains(r#""username":"Desk user""#));
        let status = status.read().unwrap();
        // 收到一次事件，成功转发 1 次（screen-2），失败 1 次（screen-1 用的仍是不可达保存地址）。
        assert_eq!(status.received_count, 1);
        assert_eq!(status.forwarded_count, 1);
        assert_eq!(status.failed_count, 1);
        assert!(status.last_error.contains("Desk"));
    }

    /// 从测试用 `TcpStream` 中读出一份完整的 HTTP 请求（读到请求头结束，
    /// 再按 Content-Length 读满请求体），供本模块的伪造设备服务器共用。
    fn read_test_http_request(stream: &mut TcpStream) -> Vec<u8> {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 2048];
        loop {
            // 持续读取并追加数据。
            let length = stream.read(&mut buffer).unwrap();
            request.extend_from_slice(&buffer[..length]);
            // 头部结束标志还没出现，继续读。
            let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
                continue;
            };
            let header_end = header_end + 4;
            // 解析出 Content-Length，判断请求体是否已经读完整。
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
        request
    }

    /// 启动一个只接受一次连接、读完完整请求后先睡眠再回 200 的测试服务器，
    /// 用来验证多设备转发是否真的并发执行（而不是排队等前一台超时/变慢）。
    fn slow_test_server(delay: Duration) -> (SocketAddr, thread::JoinHandle<()>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_test_http_request(&mut stream);
            // 读完整个请求后先睡眠指定延迟，再回复，模拟一台响应缓慢的设备。
            thread::sleep(delay);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });
        (address, handle)
    }

    // 验证转发给多台设备是真正并发执行的：两台设备各自延迟 400ms 才响应，
    // 若串行转发耗时应接近 800ms，若并发则应明显小于两倍延迟。
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

        // 记录开始时间，触发一次会同时转发给两台设备的事件。
        let started = Instant::now();
        relay_hook(
            &client,
            &data,
            &online_devices,
            &status,
            AiTool::Codex,
            "UserPromptSubmit",
            HookTransition::Display(HookBehavior::Running),
        );
        let elapsed = started.elapsed();

        server_one.join().unwrap();
        server_two.join().unwrap();
        // 两台设备都应转发成功。
        assert_eq!(status.read().unwrap().forwarded_count, 2);
        // 关键断言：总耗时应明显小于两台设备延迟之和，证明是并发而非串行转发。
        assert!(
            elapsed < delay * 2,
            "two devices answering in {delay:?} should overlap, not add up (took {elapsed:?})"
        );
    }

    // 验证切换当前设备后，profiles() 只返回新设备的 Profile（而不是混合了旧设备的）；
    // 同时验证切回旧设备后能重新看到旧设备的 Profile，且历史设备记录、用户名等不会丢失。
    #[test]
    fn switching_current_device_loads_that_devices_profiles() {
        // 用当前时间戳+进程号构造一个唯一的临时目录，避免测试间相互干扰。
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
        // 选中设备一（screen-1）并保存一个 Profile。
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

        // 切换到设备二（screen-2）。
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

        // 设备二还没有保存过任何 Profile，应为空列表。
        assert!(service.profiles().unwrap().is_empty());
        let mut studio_profile = test_profile();
        studio_profile.slot = 9;
        service.save_profile(studio_profile).unwrap();
        let saved_studio_profile = service.profiles().unwrap().remove(0);
        assert_eq!(saved_studio_profile.device_id, "screen-2");
        assert_eq!(saved_studio_profile.slot, 9);
        // 切回设备一。
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
        // 应该重新看到设备一之前保存的 Profile（槽位 1）。
        let profile = service.profiles().unwrap().remove(0);
        assert_eq!(profile.tool, AiTool::Codex);
        assert_eq!(profile.device_id, "screen-1");
        assert_eq!(profile.slot, 1);
        // 用户名在切换设备过程中应保持不变。
        assert_eq!(service.settings().unwrap().username, "Manon");
        // 历史设备列表应同时保留两台设备的记录。
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

    // 验证当前选中设备不在在线列表中时会自动切换到第一台在线设备，
    // 并且这次自动切换会被持久化（重新加载服务后仍是新设备）。
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

        // 在线列表为空：不应触发切换，仍然选中原设备。
        assert!(
            !service
                .select_first_available_device_if_needed(&[])
                .unwrap()
        );
        assert_eq!(service.settings().unwrap().device_id, "screen-1");
        // 在线列表只包含另一台设备（不含当前选中设备）：应触发切换。
        assert!(
            service
                .select_first_available_device_if_needed(std::slice::from_ref(&next))
                .unwrap()
        );
        assert_eq!(service.settings().unwrap().device_id, "screen-2");

        // 重新加载服务（模拟应用重启），验证切换结果已被持久化。
        drop(service);
        let reloaded = MonitorService::load(&app_data, &config_home).unwrap();
        assert_eq!(reloaded.settings().unwrap().device_id, "screen-2");
        drop(reloaded);
        fs::remove_dir_all(root).unwrap();
    }

    // 验证批量图片上传校验会检查列表中的每一个文件，而不是遇到第一个合法文件就通过；
    // 列表里混入一个不支持的 TIFF 格式应导致整体校验失败。
    #[test]
    fn batch_image_validation_checks_every_file_before_upload() {
        let images = vec![
            ImageUpload {
                filename: "valid.png".to_owned(),
                mime_type: "image/png".to_owned(),
                bytes: vec![1],
            },
            ImageUpload {
                filename: "invalid.tiff".to_owned(),
                mime_type: "image/tiff".to_owned(),
                bytes: vec![1],
            },
        ];

        // 即使第一个文件合法，只要列表中有一个不支持的类型，整体也应报错。
        assert_eq!(
            validate_image_uploads(&images),
            Err("invalid.tiff 不是支持的 BMP、JPEG、GIF、PNG 或 WebP 图片".to_owned())
        );
    }

    #[test]
    fn batch_image_validation_accepts_all_supported_upload_types() {
        let images = [
            ("legacy.bmp", "image/bmp"),
            ("photo.jpg", "image/jpeg"),
            ("photo.jpeg", "image/jpeg"),
            ("moving.gif", "image/gif"),
            ("graphic.png", "image/png"),
            ("modern.webp", "image/webp"),
        ]
        .map(|(filename, mime_type)| ImageUpload {
            filename: filename.to_owned(),
            mime_type: mime_type.to_owned(),
            bytes: vec![1],
        });

        assert!(validate_image_uploads(&images).is_ok());
    }

    // 验证空的图片选择会被拒绝，提示用户先选择图片。
    #[test]
    fn batch_image_validation_rejects_an_empty_selection() {
        assert_eq!(
            validate_image_uploads(&[]),
            Err("请选择要上传的图片".to_owned())
        );
    }

    // 验证 remote_image_url 会对文件名做正确的 URL 编码（含中文、空格、# 等特殊字符），
    // 同时验证包含路径穿越（"../secret"）的文件名会被拒绝。
    #[test]
    fn remote_image_url_encodes_one_filename_path_segment() {
        let url = remote_image_url("http://192.168.50.20:8080", "状态 图片 #1.gif").unwrap();

        assert_eq!(
            url.as_str(),
            "http://192.168.50.20:8080/api/images/%E7%8A%B6%E6%80%81%20%E5%9B%BE%E7%89%87%20%231.gif"
        );
        assert!(remote_image_url("http://192.168.50.20:8080", "../secret").is_err());
    }

    // 验证 IPv4 候选地址排序时排在 IPv6 之前。
    #[test]
    fn discovery_prefers_ipv4_candidates_before_ipv6() {
        let mut urls = [
            "http://[fd00::20]:8080".to_owned(),
            "http://192.168.50.20:8080".to_owned(),
        ];

        urls.sort_by_key(|url| candidate_url_priority(url));

        assert_eq!(urls[0], "http://192.168.50.20:8080");
    }

    // 测试辅助函数：快速构造一个只有单个 base_url 的发现候选。
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

    // 验证合并 mDNS 与 UDP 两路发现结果时，只在其中一路出现的设备不会丢失，
    // 同一设备在两路都出现时其候选地址会被正确取并集。
    #[test]
    fn merging_discovery_sources_keeps_devices_only_seen_on_one_protocol() {
        // Regression test: two physical devices both running the app, but only
        // one of them answers mDNS reliably (e.g. multicast dropped by the AP)
        // while both answer UDP broadcast. The old short-circuit logic
        // (`if mdns non-empty, ignore udp entirely`) silently dropped the
        // second device from the list.
        // 只出现在 mDNS 一路的设备。
        let mdns_only = discovery_candidate(
            "device-a",
            "Living Room",
            "http://192.168.1.10:8080",
            DiscoverySource::Mdns,
        );
        // 两路都会出现的设备（mDNS 那份）。
        let mdns_and_udp = discovery_candidate(
            "device-c",
            "Kitchen",
            "http://192.168.1.30:8080",
            DiscoverySource::Mdns,
        );
        // 只出现在 UDP 一路的设备。
        let udp_only = discovery_candidate(
            "device-b",
            "Bedroom",
            "http://192.168.1.20:8080",
            DiscoverySource::UdpBroadcast,
        );
        // 两路都出现的设备（UDP 那份），但带了一个 mDNS 那份没有的额外地址。
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
        // 三台设备（仅 mDNS、仅 UDP、两路都有）都应存活在合并结果中。
        assert_eq!(
            ids.len(),
            3,
            "mDNS-only, UDP-only and both-source devices must all survive the merge, got {ids:?}"
        );
        assert!(ids.contains(&"device-a"));
        assert!(ids.contains(&"device-b"));

        // 两路都出现的设备（Kitchen），其候选地址应该是两路地址的并集。
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

    // 验证设备需要连续两轮未被发现才会真正从在线快照移除（去抖机制），
    // 单轮偶发漏报应该被容忍，不影响设备继续显示在线。
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

        // 第一轮只发现了 device_a：device_b 缺席 1 次，但未达到移除阈值，仍应保留在结果中。
        let after_one_miss =
            stabilize_discovered_devices(&previous, vec![device_a.clone()], &mut missed_scans);
        assert_eq!(after_one_miss.len(), 2);

        // 第二轮仍只发现 device_a：device_b 累计缺席 2 次，达到阈值，应从结果中移除。
        let after_two_misses = stabilize_discovered_devices(
            &after_one_miss,
            vec![device_a.clone()],
            &mut missed_scans,
        );
        assert_eq!(after_two_misses, vec![device_a]);
        assert_eq!(missed_scans.get("device-b"), Some(&2));
    }

    // 验证发现间隔的默认值、非法值拒绝以及保存后立即生效（无需重启服务）。
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

        // 默认间隔应为 1 分钟。
        assert_eq!(service.discovery_interval(), Duration::from_mins(1));
        // 0 分钟是非法值，应被拒绝。
        assert!(service.save_discovery_interval(0).is_err());

        // 保存 15 分钟后，无需重启服务即可立即读到新值。
        let updated = service.save_discovery_interval(15).unwrap();
        assert_eq!(updated.discovery_interval_minutes, 15);
        assert_eq!(service.discovery_interval(), Duration::from_mins(15));
    }

    // 验证 discovery_base_url 对 IPv4 与 IPv6（链路本地）地址都能正确拼出可直接探测的 URL。
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

    // 验证根据 IP 和子网掩码计算定向广播地址的正确性（不同掩码长度）。
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

    // 验证 UDP 发现响应解析：base_url 使用的是数据包实际来源 IP（而非响应体里可能伪造的字段），
    // 端口使用响应体中通告的端口；同时验证中文名称能被正确解析。
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

    // 验证非法元数据（id 为空、端口为 0）都会被拒绝解析，返回 None。
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

    // 端到端验证 UDP 发现的完整往返流程：本地起一个模拟设备 UDP 服务器，
    // 验证收到的探测报文与协议约定的固定内容一致（与 Android 端协议匹配），
    // 并验证响应能被正确解析为发现候选。
    #[test]
    fn udp_discovery_round_trip_matches_the_android_protocol() {
        let server = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let port = server.local_addr().unwrap().port();
        // 模拟设备端：接收探测请求，校验报文内容，回复设备信息。
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

        // 用回环地址作为广播目标（测试环境无法真正广播），触发一次完整发现。
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

        // 应该正确发现到这一台模拟设备。
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].device.id, "device-loopback");
        assert_eq!(candidates[0].device.base_url, "http://127.0.0.1:8080");
    }

    // 验证保存 Profile 时若持久化到磁盘失败（此处故意把 data_path 指向一个目录而非文件，
    // 制造写入失败），不会产生"内存已更新但磁盘未写入"的不一致状态：
    // save_profile 应返回错误，且内存中的 profiles 仍应为空，Hook 配置文件也不应被生成。
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
        // 故意让 data_path 指向一个目录（而非文件），这样写入配置时必然失败。
        let invalid_data_path = root.join("data-path-is-a-directory");
        fs::create_dir_all(&invalid_data_path).unwrap();

        // 手工构造 MonitorService（而非走 load()），以便注入这个必然失败的 data_path。
        let service = MonitorService {
            client: Client::new(),
            data_path: invalid_data_path,
            default_hook_config_directories: HookConfigDirectories {
                codex: config_home.join(".codex").to_string_lossy().into_owned(),
                claude_code: config_home.join(".claude").to_string_lossy().into_owned(),
                cursor: config_home.join(".cursor").to_string_lossy().into_owned(),
                open_code: config_home
                    .join(".config/opencode")
                    .to_string_lossy()
                    .into_owned(),
                work_buddy: config_home
                    .join(".workbuddy")
                    .to_string_lossy()
                    .into_owned(),
                harness: config_home
                    .join("Library/Application Support/Harness")
                    .to_string_lossy()
                    .into_owned(),
                open_claw: config_home.join(".openclaw").to_string_lossy().into_owned(),
                code_buddy: config_home
                    .join(".codebuddy")
                    .to_string_lossy()
                    .into_owned(),
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

        // 保存应失败，且没有留下任何副作用：既没写 hooks.json，内存里的 profiles 也仍为空。
        assert!(result.is_err());
        assert!(!config_home.join(".codex/hooks.json").exists());
        assert!(service.profiles().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    // 验证自定义 Hook 配置目录能被正确持久化，并且 write_hook_config 会写入到该自定义目录
    // 而非默认目录；重新加载服务后自定义目录设置依然生效；清空自定义目录后能恢复默认目录。
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
        // 记录下切换自定义目录之前系统探测到的默认目录，后面用于验证"清空自定义目录后能恢复默认值"。
        let detected_directory = service
            .hook_config_locations()
            .unwrap()
            .into_iter()
            .find(|item| item.tool == AiTool::Codex)
            .unwrap()
            .directory;

        // 保存自定义目录。
        let location = service
            .save_hook_config_directory(AiTool::Codex, &custom_directory.to_string_lossy())
            .unwrap();

        // 保存后应标记为自定义，且配置文件路径应指向自定义目录下的 hooks.json。
        assert!(location.is_custom);
        assert_eq!(
            PathBuf::from(&location.config_path),
            custom_directory.join("hooks.json")
        );
        // Hooks 不依赖设备展示 Profile；首次配置时可以先完成写入。
        assert!(service.profiles().unwrap().is_empty());
        assert!(!custom_directory.join("hooks.json").exists());
        service.write_hook_config(AiTool::Codex).unwrap();
        // 写入后应出现在自定义目录，而不是默认目录。
        assert!(custom_directory.join("hooks.json").exists());
        assert!(!config_home.join(".codex/hooks.json").exists());
        // 重新加载服务（模拟重启），自定义目录设置应仍然生效。
        let reloaded = MonitorService::load(&app_data, &config_home).unwrap();
        let reloaded_location = reloaded
            .hook_config_locations()
            .unwrap()
            .into_iter()
            .find(|item| item.tool == AiTool::Codex)
            .unwrap();
        assert_eq!(reloaded_location.directory, location.directory);
        assert!(reloaded_location.is_custom);

        // 传入空字符串应恢复为默认探测目录，且不再标记为自定义。
        let default_location = reloaded
            .save_hook_config_directory(AiTool::Codex, "")
            .unwrap();
        assert!(!default_location.is_custom);
        assert_eq!(default_location.directory, detected_directory);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn open_claw_plugin_files_are_validated_as_one_managed_set() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ai-monitor-openclaw-plugin-{}-{unique}",
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
        let mut profile = test_profile();
        profile.tool = AiTool::OpenClaw;
        service.save_profile(profile).unwrap();
        let plugin_root = root.join("openclaw");
        service
            .save_hook_config_directory(AiTool::OpenClaw, &plugin_root.to_string_lossy())
            .unwrap();

        // 任一辅助文件已被用户占用时，主入口和其他文件都不应提前写入。
        let package_path = plugin_root.join("extensions/aimonitor/package.json");
        fs::create_dir_all(package_path.parent().unwrap()).unwrap();
        fs::write(&package_path, r#"{"name":"unrelated"}"#).unwrap();
        assert!(service.write_hook_config(AiTool::OpenClaw).is_err());
        assert!(!plugin_root.join("extensions/aimonitor/index.mjs").exists());
        assert!(
            !plugin_root
                .join("extensions/aimonitor/openclaw.plugin.json")
                .exists()
        );

        fs::remove_file(&package_path).unwrap();
        let result = service.write_hook_config(AiTool::OpenClaw).unwrap();
        assert!(result.config_changed);
        assert!(result.requires_review);
        assert!(result.restart_required);
        assert!(plugin_root.join("extensions/aimonitor/index.mjs").exists());
        assert!(
            plugin_root
                .join("extensions/aimonitor/openclaw.plugin.json")
                .exists()
        );
        assert!(package_path.exists());

        fs::remove_dir_all(root).unwrap();
    }

    // 验证保存 Hook 配置目录时会拒绝相对路径，也会拒绝指向一个已存在普通文件（而非目录）的路径。
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

        // 相对路径应被拒绝。
        assert!(
            service
                .save_hook_config_directory(AiTool::Cursor, "relative/path")
                .is_err()
        );
        // 指向一个已存在的普通文件（非目录）也应被拒绝。
        assert!(
            service
                .save_hook_config_directory(AiTool::ClaudeCode, &file_path.to_string_lossy())
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
