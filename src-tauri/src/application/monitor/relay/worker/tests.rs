use super::super::status::record_hook_results_with_accounting;
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
use std::{
    io::ErrorKind,
    io::Write,
    net::{Ipv4Addr, TcpListener},
    sync::mpsc as std_mpsc,
    thread,
    time::Duration,
};

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
        Some(HookTransition::Release),
        2,
        &[],
        false,
    );

    let status = status.read().unwrap();
    assert_eq!(status.received_count, 0);
    assert_eq!(status.pending_count, 7);
    assert_eq!(status.forwarded_count, 2);
    assert_eq!(
        status.last_event,
        Some(HookRelayLastEvent::Release {
            tool: AiTool::Codex,
            hook_type: "SessionTimeout".to_owned(),
        })
    );
}

#[test]
fn unsupported_hook_preserves_the_last_transition_and_records_failure() {
    let last_event = HookRelayLastEvent::Display {
        tool: AiTool::Codex,
        hook_type: "UserPromptSubmit".to_owned(),
        behavior: HookBehavior::Running,
    };
    let status = Arc::new(RwLock::new(HookRelayStatus {
        pending_count: 1,
        last_event: Some(last_event.clone()),
        ..HookRelayStatus::default()
    }));
    let data = Arc::new(RwLock::new(SavedMonitorData::default()));
    let online_devices = Arc::new(RwLock::new(Vec::new()));
    let (sender, receiver) = mpsc::channel(1);
    let (_snapshot_sender, snapshot_receiver) = watch::channel(0);
    spawn_hook_worker(
        &reqwest::blocking::Client::new(),
        receiver,
        snapshot_receiver,
        &data,
        &online_devices,
        Arc::clone(&status),
    );

    sender
        .try_send(IncomingHookEvent {
            tool: AiTool::Codex,
            hook_type: "UnknownHook".to_owned(),
            session_id: None,
            turn_id: None,
            status: None,
        })
        .unwrap();
    drop(sender);

    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline && status.read().unwrap().received_count == 0 {
        thread::sleep(Duration::from_millis(10));
    }
    let status = status.read().unwrap();
    assert_eq!(status.received_count, 1);
    assert_eq!(status.pending_count, 0);
    assert_eq!(status.failed_count, 1);
    assert_eq!(status.last_event, Some(last_event));
    assert_eq!(status.last_error, "不支持的 Hook 类型：UnknownHook");
}

#[test]
fn newly_online_device_replays_the_current_display_without_another_hook() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let (request_sender, request_receiver) = std_mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = String::from_utf8(read_test_http_request(&mut stream)).unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        request_sender.send(request).unwrap();
    });
    let data = Arc::new(RwLock::new(SavedMonitorData {
        client_id: "test-client".to_owned(),
        settings: MonitorSettings {
            username: "Manon".to_owned(),
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
    let online_devices = Arc::new(RwLock::new(Vec::new()));
    let status = Arc::new(RwLock::new(HookRelayStatus {
        pending_count: 1,
        ..HookRelayStatus::default()
    }));
    let (sender, receiver) = mpsc::channel(1);
    let (snapshot_sender, snapshot_receiver) = watch::channel(0);
    spawn_hook_worker(
        &reqwest::blocking::Client::new(),
        receiver,
        snapshot_receiver,
        &data,
        &online_devices,
        Arc::clone(&status),
    );

    sender
        .try_send(IncomingHookEvent {
            tool: AiTool::Codex,
            hook_type: "UserPromptSubmit".to_owned(),
            session_id: Some("offline-session".to_owned()),
            turn_id: Some("turn-1".to_owned()),
            status: None,
        })
        .unwrap();
    let initial_deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < initial_deadline && status.read().unwrap().received_count == 0 {
        thread::sleep(Duration::from_millis(10));
    }
    {
        let status = status.read().unwrap();
        assert_eq!(status.received_count, 1);
        assert_eq!(status.forwarded_count, 0);
        assert_eq!(status.failed_count, 0);
        assert_eq!(status.pending_count, 0);
    }

    *online_devices.write().unwrap() = vec![DiscoveredMonitorDevice {
        id: "screen-1".to_owned(),
        name: "Desk".to_owned(),
        api_version: "1".to_owned(),
        base_url: format!("http://{address}"),
        path: DEFAULT_DEVICE_API_PATH.to_owned(),
        discovery_source: DiscoverySource::Mdns,
    }];
    snapshot_sender.send_replace(1);

    let request = request_receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    assert!(request.starts_with("POST /api/slots/1 HTTP/1.1"));
    assert!(request.contains(r#""behavior":"running""#));
    server.join().unwrap();

    let replay_deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < replay_deadline && status.read().unwrap().forwarded_count == 0 {
        thread::sleep(Duration::from_millis(10));
    }
    let status = status.read().unwrap();
    assert_eq!(status.received_count, 1);
    assert_eq!(status.forwarded_count, 1);
    assert_eq!(status.failed_count, 0);
    assert_eq!(status.suppressed_count, 0);
    assert_eq!(status.pending_count, 0);
    assert_eq!(
        status.last_event,
        Some(HookRelayLastEvent::Display {
            tool: AiTool::Codex,
            hook_type: DEVICE_ONLINE_REPLAY_HOOK_TYPE.to_owned(),
            behavior: HookBehavior::Running,
        })
    );
}

#[test]
fn replay_targets_only_new_reconnected_or_address_changed_devices() {
    let previous = OnlineDeviceRoutes::from([
        (
            "unchanged".to_owned(),
            ("http://127.0.0.1:1".to_owned(), "/api/device".to_owned()),
        ),
        (
            "changed".to_owned(),
            ("http://127.0.0.1:2".to_owned(), "/api/device".to_owned()),
        ),
        (
            "offline".to_owned(),
            ("http://127.0.0.1:3".to_owned(), "/api/device".to_owned()),
        ),
    ]);
    let current = OnlineDeviceRoutes::from([
        (
            "unchanged".to_owned(),
            ("http://127.0.0.1:1".to_owned(), "/api/device".to_owned()),
        ),
        (
            "changed".to_owned(),
            ("http://127.0.0.1:4".to_owned(), "/api/device".to_owned()),
        ),
        (
            "reconnected".to_owned(),
            ("http://127.0.0.1:3".to_owned(), "/api/device".to_owned()),
        ),
    ]);

    assert_eq!(
        device_ids_requiring_replay(&previous, &current),
        vec!["changed".to_owned(), "reconnected".to_owned()]
    );
}

#[test]
fn passthrough_tools_do_not_replay_on_device_snapshot_changes() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let mut profile = test_profile();
    profile.tool = AiTool::Hermes;
    let data = Arc::new(RwLock::new(SavedMonitorData {
        client_id: "test-client".to_owned(),
        settings: MonitorSettings {
            username: "Manon".to_owned(),
            ..MonitorSettings::default()
        },
        devices: vec![MonitorDeviceRoute {
            base_url: format!("http://{address}"),
            device_id: "screen-1".to_owned(),
            device_name: "Desk".to_owned(),
        }],
        profiles: vec![profile],
        hook_config_directories: HookConfigDirectories::default(),
    }));
    let online_devices = Arc::new(RwLock::new(Vec::new()));
    let status = Arc::new(RwLock::new(HookRelayStatus::default()));
    let (sender, receiver) = mpsc::channel(1);
    let (snapshot_sender, snapshot_receiver) = watch::channel(0);
    spawn_hook_worker(
        &reqwest::blocking::Client::new(),
        receiver,
        snapshot_receiver,
        &data,
        &online_devices,
        Arc::clone(&status),
    );

    sender
        .try_send(IncomingHookEvent {
            tool: AiTool::Hermes,
            hook_type: "pre_llm_call".to_owned(),
            session_id: Some("session-1".to_owned()),
            turn_id: Some("turn-1".to_owned()),
            status: None,
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline && status.read().unwrap().received_count == 0 {
        thread::sleep(Duration::from_millis(10));
    }

    *online_devices.write().unwrap() = vec![DiscoveredMonitorDevice {
        id: "screen-1".to_owned(),
        name: "Desk".to_owned(),
        api_version: "1".to_owned(),
        base_url: format!("http://{address}"),
        path: DEFAULT_DEVICE_API_PATH.to_owned(),
        discovery_source: DiscoverySource::Mdns,
    }];
    snapshot_sender.send_replace(1);
    thread::sleep(Duration::from_millis(150));

    listener.set_nonblocking(true).unwrap();
    assert_eq!(listener.accept().unwrap_err().kind(), ErrorKind::WouldBlock);
    let status = status.read().unwrap();
    assert_eq!(status.received_count, 1);
    assert_eq!(status.forwarded_count, 0);
    assert_eq!(status.failed_count, 0);
}
