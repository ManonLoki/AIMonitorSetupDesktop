use std::{
    io::Write,
    net::{Ipv4Addr, TcpListener},
    time::{Duration, Instant},
};

use super::*;
use crate::{
    application::monitor::{
        DEFAULT_DEVICE_API_PATH, HookRelayLastEvent,
        test_support::{read_test_http_request, slow_test_server, test_profile},
    },
    domain::monitor::{
        DiscoverySource, HookConfigDirectories, MonitorDeviceRoute, MonitorSettings,
    },
};

#[test]
fn target_selection_intersects_profiles_with_the_online_snapshot() {
    let mut offline_profile = test_profile();
    offline_profile.device_id = "offline-screen".to_owned();
    let data = Arc::new(RwLock::new(SavedMonitorData {
        client_id: "test-client".to_owned(),
        settings: MonitorSettings::default(),
        devices: vec![
            MonitorDeviceRoute {
                base_url: "http://127.0.0.1:1".to_owned(),
                device_id: "screen-1".to_owned(),
                device_name: "Online".to_owned(),
            },
            MonitorDeviceRoute {
                base_url: "http://127.0.0.1:2".to_owned(),
                device_id: "offline-screen".to_owned(),
                device_name: "Offline".to_owned(),
            },
        ],
        profiles: vec![test_profile(), offline_profile],
        hook_config_directories: HookConfigDirectories::default(),
    }));
    let online_devices = Arc::new(RwLock::new(vec![DiscoveredMonitorDevice {
        id: "screen-1".to_owned(),
        name: "Online".to_owned(),
        api_version: "1".to_owned(),
        base_url: "http://127.0.0.1:3".to_owned(),
        path: DEFAULT_DEVICE_API_PATH.to_owned(),
        discovery_source: DiscoverySource::Mdns,
    }]));

    let (configured_count, target_ids) =
        configured_online_target_ids(&data, &online_devices, AiTool::Codex).unwrap();

    assert_eq!(configured_count, 2);
    assert_eq!(target_ids, vec!["screen-1"]);
}

#[test]
fn target_that_went_offline_while_queued_is_skipped_before_http() {
    let data = Arc::new(RwLock::new(SavedMonitorData {
        client_id: "test-client".to_owned(),
        settings: MonitorSettings {
            username: "Manon".to_owned(),
            ..MonitorSettings::default()
        },
        devices: vec![MonitorDeviceRoute {
            base_url: "http://127.0.0.1:1".to_owned(),
            device_id: "screen-1".to_owned(),
            device_name: "Desk".to_owned(),
        }],
        profiles: vec![test_profile()],
        hook_config_directories: HookConfigDirectories::default(),
    }));
    let online_devices = Arc::new(RwLock::new(Vec::new()));

    let forwarded = forward_hook_to_target(
        &reqwest::blocking::Client::new(),
        &data,
        &online_devices,
        AiTool::Codex,
        HookTransition::Display(HookBehavior::Running),
        "screen-1",
    )
    .unwrap();

    assert!(!forwarded);
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
        client_id: "test-client".to_owned(),
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
    let online_devices = Arc::new(RwLock::new(vec![DiscoveredMonitorDevice {
        id: "screen-1".to_owned(),
        name: "Desk".to_owned(),
        api_version: "1".to_owned(),
        base_url: format!("http://{address}"),
        path: DEFAULT_DEVICE_API_PATH.to_owned(),
        discovery_source: DiscoverySource::Mdns,
    }]));

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
    assert!(request.contains(r#""clientId":"test-client""#));
    assert!(request.contains(r#""username":"Manon""#));
    assert!(request.contains(r#""aiName":"Codex""#));
    assert!(request.contains(r#""behavior":"running""#));
    assert!(request.contains(r#""image":"running.gif""#));
    let status = status.read().unwrap();
    // 断言中继状态被正确更新：收到 1 次，转发成功 1 次，最近行为为 Running，无错误。
    assert_eq!(status.received_count, 1);
    assert_eq!(status.forwarded_count, 1);
    assert_eq!(
        status.last_event,
        Some(HookRelayLastEvent::Display {
            tool: AiTool::Codex,
            hook_type: "UserPromptSubmit".to_owned(),
            behavior: HookBehavior::Running,
        })
    );
    assert!(status.last_error.is_empty());
}

// 验证转发时只使用在线快照中的设备及最新地址：screen-1 虽有历史记录和
// Profile，但不在线；screen-2 在线并由发现快照提供可用新地址。
#[test]
fn relay_filters_offline_routes_and_uses_the_online_address() {
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
        client_id: "test-client".to_owned(),
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
    // 收到一次事件，只成功转发给 screen-2；离线的 screen-1 不算转发失败。
    assert_eq!(status.received_count, 1);
    assert_eq!(status.forwarded_count, 1);
    assert_eq!(status.failed_count, 0);
    assert!(status.last_error.is_empty());
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
        client_id: "test-client".to_owned(),
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
    let online_devices = Arc::new(RwLock::new(vec![
        DiscoveredMonitorDevice {
            id: "screen-1".to_owned(),
            name: "Desk".to_owned(),
            api_version: "1".to_owned(),
            base_url: format!("http://{address_one}"),
            path: DEFAULT_DEVICE_API_PATH.to_owned(),
            discovery_source: DiscoverySource::Mdns,
        },
        DiscoveredMonitorDevice {
            id: "screen-2".to_owned(),
            name: "Studio".to_owned(),
            api_version: "1".to_owned(),
            base_url: format!("http://{address_two}"),
            path: DEFAULT_DEVICE_API_PATH.to_owned(),
            discovery_source: DiscoverySource::Mdns,
        },
    ]));
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
