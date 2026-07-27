use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, UdpSocket},
    thread,
    time::Duration,
};

use mdns_sd::{InterfaceId, ScopedIp, ScopedIpV4};

use super::*;
use crate::domain::monitor::{DiscoveredMonitorDevice, DiscoverySource};

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
    let after_two_misses =
        stabilize_discovered_devices(&after_one_miss, vec![device_a.clone()], &mut missed_scans);
    assert_eq!(after_two_misses, vec![device_a]);
    assert_eq!(missed_scans.get("device-b"), Some(&2));
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
