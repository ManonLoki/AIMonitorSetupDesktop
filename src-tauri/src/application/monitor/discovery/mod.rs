// mDNS/UDP 设备发现的纯算法部分：候选地址合并、优先级排序、在线快照去抖。
// `MonitorService` 侧的编排逻辑（发起 mDNS 查询、调用这些函数）位于
// `service_discovery.rs`，二者同为 `application::monitor` 的子模块。
use std::{
    collections::{HashMap, HashSet},
    io::ErrorKind,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    thread,
    time::{Duration, Instant},
};

use if_addrs::{IfAddr, IfOperStatus};
use mdns_sd::ScopedIp;
use serde::Deserialize;

use super::{
    DEFAULT_DEVICE_API_PATH, DISCOVERY_MISSES_BEFORE_REMOVAL, UDP_DISCOVERY_PORT,
    UDP_DISCOVERY_REQUEST, UDP_DISCOVERY_TIMEOUT, UDP_RESPONSE_MAX_BYTES,
};
use crate::domain::monitor::{DiscoveredMonitorDevice, DiscoverySource, normalize_base_url};

// 两轮 UDP 探测报文之间的发送间隔，给偶发丢包留出重试窗口。
const UDP_BROADCAST_RETRY_INTERVAL: Duration = Duration::from_millis(75);
// 一轮轮询未收到任何响应时的休眠间隔，避免非阻塞 socket 轮询忙等占满 CPU。
const UDP_POLL_IDLE_BACKOFF: Duration = Duration::from_millis(10);

#[cfg(test)]
mod tests;

/// 一次发现命中的设备，可能同时拥有多个候选地址（IPv4/IPv6、多网卡）；
/// 连接测试按 `candidate_url_priority` 排序后依次尝试，取第一个可达的。
#[derive(Clone, Debug)]
pub(crate) struct DiscoveryCandidate {
    // 已发现的设备领域实体。
    pub(super) device: DiscoveredMonitorDevice,
    // 该设备所有候选的 base url（按优先级排序前的原始集合）。
    pub(super) base_urls: Vec<String>,
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
pub(super) fn discovery_base_url(address: &ScopedIp, port: u16) -> Option<String> {
    match address {
        // IPv4 地址直接拼接。
        ScopedIp::V4(address) => Some(format!("http://{}:{port}", address.addr())),
        // reqwest 0.12 的 URL 传输边界无法表示 RFC 6874 zone identifier；
        // 链路本地 IPv6 缺少作用域又无法路由，因此不要生成必然探测失败的候选。
        ScopedIp::V6(address) if address.addr().is_unicast_link_local() => None,
        // 其他 IPv6 地址（非链路本地）直接拼接，无需作用域 ID。
        ScopedIp::V6(address) => Some(format!("http://[{}]:{port}", address.addr())),
        // 未知地址类型不生成候选。
        _ => None,
    }
}

/// IPv4/主机名优先于普通 IPv6；无法解析、非 HTTP(S) 或当前传输层无法
/// 表示的链路本地 IPv6 候选排在最后。
pub(super) fn candidate_url_priority(base_url: &str) -> u8 {
    // `normalize_base_url` 已经完成了全部校验（协议、主机、链路本地 IPv6
    // 拒绝），返回值固定是 `{scheme}://{authority}` 形式；scheme 只会是
    // http/https 两种字面量，据此直接判断 authority 是否以 `[` 开头
    // （IPv6 字面量地址），无需再重新解析一次 URI。
    let Ok(normalized) = normalize_base_url(base_url) else {
        return 2;
    };
    let is_ipv6_literal = normalized
        .strip_prefix("http://")
        .or_else(|| normalized.strip_prefix("https://"))
        .is_some_and(|authority| authority.starts_with('['));

    u8::from(is_ipv6_literal)
}

// 通过 UDP 广播方式发现设备：先枚举本机所有可用网卡的广播目标，再逐个发送探测报文。
pub(super) fn discover_udp_candidates() -> Result<Vec<DiscoveryCandidate>, String> {
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

    // 发送两轮探测报文，提高在丢包网络下的命中率。
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
        thread::sleep(UDP_BROADCAST_RETRY_INTERVAL);
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
            thread::sleep(UDP_POLL_IDLE_BACKOFF);
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
pub(super) fn merge_discovery_candidates(
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
pub(super) fn stabilize_discovered_devices(
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
