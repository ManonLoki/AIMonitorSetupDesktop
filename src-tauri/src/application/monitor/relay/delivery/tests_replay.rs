use std::{
    io::{ErrorKind, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener},
    sync::{Arc, RwLock},
    thread,
    time::{Duration, Instant},
};

use super::*;
use crate::{
    application::monitor::{
        DEFAULT_DEVICE_API_PATH,
        test_support::{read_test_http_request, test_profile},
    },
    domain::monitor::{
        DiscoverySource, HookBehavior, HookConfigDirectories, MonitorDeviceRoute, MonitorSettings,
    },
};

fn online_device(id: &str, address: SocketAddr) -> DiscoveredMonitorDevice {
    DiscoveredMonitorDevice {
        id: id.to_owned(),
        name: id.to_owned(),
        api_version: "1".to_owned(),
        base_url: format!("http://{address}"),
        path: DEFAULT_DEVICE_API_PATH.to_owned(),
        discovery_source: DiscoverySource::Mdns,
    }
}

#[test]
fn replay_is_scoped_to_the_devices_that_changed_availability() {
    let existing_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let replay_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let existing_address = existing_listener.local_addr().unwrap();
    let replay_address = replay_listener.local_addr().unwrap();
    let replay_server = thread::spawn(move || {
        let (mut stream, _) = replay_listener.accept().unwrap();
        let request = String::from_utf8(read_test_http_request(&mut stream)).unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        request
    });

    let mut replay_profile = test_profile();
    replay_profile.device_id = "reconnected".to_owned();
    replay_profile.slot = 2;
    let data = Arc::new(RwLock::new(SavedMonitorData {
        client_id: "test-client".to_owned(),
        settings: MonitorSettings {
            username: "Manon".to_owned(),
            ..MonitorSettings::default()
        },
        devices: vec![
            MonitorDeviceRoute {
                base_url: format!("http://{existing_address}"),
                device_id: "screen-1".to_owned(),
                device_name: "Existing".to_owned(),
            },
            MonitorDeviceRoute {
                base_url: format!("http://{replay_address}"),
                device_id: "reconnected".to_owned(),
                device_name: "Reconnected".to_owned(),
            },
        ],
        profiles: vec![test_profile(), replay_profile],
        hook_config_directories: HookConfigDirectories::default(),
    }));
    let online_devices = Arc::new(RwLock::new(vec![
        online_device("screen-1", existing_address),
        online_device("reconnected", replay_address),
    ]));
    let status = Arc::new(RwLock::new(HookRelayStatus::default()));
    let mut scheduler = DeliveryScheduler::new(
        &reqwest::blocking::Client::new(),
        &data,
        &online_devices,
        Arc::clone(&status),
    );

    scheduler.enqueue_to_devices(
        &PendingHookRelay {
            tool: AiTool::Codex,
            hook_type: "DeviceOnlineReplay".to_owned(),
            transition: HookTransition::Display(HookBehavior::Running),
            counts_as_hook: false,
        },
        &["reconnected".to_owned()],
    );

    let request = replay_server.join().unwrap();
    assert!(request.starts_with("POST /api/slots/2 HTTP/1.1"));
    existing_listener.set_nonblocking(true).unwrap();
    assert_eq!(
        existing_listener.accept().unwrap_err().kind(),
        ErrorKind::WouldBlock
    );

    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline && status.read().unwrap().forwarded_count == 0 {
        thread::sleep(Duration::from_millis(10));
    }
    let status = status.read().unwrap();
    assert_eq!(status.received_count, 0);
    assert_eq!(status.pending_count, 0);
    assert_eq!(status.forwarded_count, 1);
    assert_eq!(status.failed_count, 0);
}
