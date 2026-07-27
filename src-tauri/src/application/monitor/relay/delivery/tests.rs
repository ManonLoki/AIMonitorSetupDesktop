use std::{
    io::Write,
    net::{Ipv4Addr, SocketAddr, TcpListener},
    sync::mpsc,
    time::{Duration, Instant},
};

use super::*;
use crate::{
    application::monitor::{
        DEFAULT_DEVICE_API_PATH,
        test_support::{read_test_http_request, test_profile},
    },
    domain::monitor::{
        DiscoverySource, HookConfigDirectories, MonitorDeviceRoute, MonitorSettings,
    },
};

fn two_device_data(slow: SocketAddr, fast: SocketAddr) -> SavedMonitorData {
    let mut slow_profile = test_profile();
    slow_profile.device_id = "slow-screen".to_owned();
    let mut fast_profile = test_profile();
    fast_profile.device_id = "fast-screen".to_owned();
    fast_profile.slot = 2;
    SavedMonitorData {
        settings: MonitorSettings {
            username: "Manon".to_owned(),
            ..MonitorSettings::default()
        },
        devices: vec![
            MonitorDeviceRoute {
                base_url: format!("http://{slow}"),
                device_id: "slow-screen".to_owned(),
                device_name: "Slow".to_owned(),
            },
            MonitorDeviceRoute {
                base_url: format!("http://{fast}"),
                device_id: "fast-screen".to_owned(),
                device_name: "Fast".to_owned(),
            },
        ],
        profiles: vec![slow_profile, fast_profile],
        hook_config_directories: HookConfigDirectories::default(),
    }
}

fn relay(hook_type: &str, behavior: HookBehavior) -> PendingHookRelay {
    PendingHookRelay {
        tool: AiTool::Codex,
        hook_type: hook_type.to_owned(),
        transition: HookTransition::Display(behavior),
        counts_as_hook: true,
    }
}

fn online_device(id: &str, name: &str, address: SocketAddr) -> DiscoveredMonitorDevice {
    DiscoveredMonitorDevice {
        id: id.to_owned(),
        name: name.to_owned(),
        api_version: "1".to_owned(),
        base_url: format!("http://{address}"),
        path: DEFAULT_DEVICE_API_PATH.to_owned(),
        discovery_source: DiscoverySource::Mdns,
    }
}

#[test]
fn slow_device_does_not_hide_asking_state_from_fast_device() {
    let slow_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let fast_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let slow_address = slow_listener.local_addr().unwrap();
    let fast_address = fast_listener.local_addr().unwrap();
    let (slow_started_tx, slow_started_rx) = mpsc::channel();
    let (release_slow_tx, release_slow_rx) = mpsc::channel();
    let slow_server = std::thread::spawn(move || {
        let (mut stream, _) = slow_listener.accept().unwrap();
        read_test_http_request(&mut stream);
        slow_started_tx.send(()).unwrap();
        release_slow_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        drop(stream);
        let (mut stream, _) = slow_listener.accept().unwrap();
        read_test_http_request(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
    });
    let (fast_requests_tx, fast_requests_rx) = mpsc::channel();
    let fast_server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = fast_listener.accept().unwrap();
            let request = String::from_utf8(read_test_http_request(&mut stream)).unwrap();
            fast_requests_tx.send(request).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        }
    });

    let data = Arc::new(RwLock::new(two_device_data(slow_address, fast_address)));
    let online_devices = Arc::new(RwLock::new(vec![
        online_device("slow-screen", "Slow", slow_address),
        online_device("fast-screen", "Fast", fast_address),
    ]));
    let status = Arc::new(RwLock::new(HookRelayStatus {
        pending_count: 2,
        ..HookRelayStatus::default()
    }));
    let mut scheduler = DeliveryScheduler::new(
        &reqwest::blocking::Client::new(),
        &data,
        &online_devices,
        Arc::clone(&status),
    );

    scheduler.enqueue(&relay("UserPromptSubmit", HookBehavior::Running));
    slow_started_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    let running = fast_requests_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    assert!(running.contains(r#""behavior":"running""#));

    scheduler.enqueue(&relay("PermissionRequest", HookBehavior::Asking));
    let asking_started = Instant::now();
    let asking = fast_requests_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    assert!(asking.contains(r#""behavior":"asking""#));
    assert!(asking_started.elapsed() < Duration::from_secs(1));

    release_slow_tx.send(()).unwrap();
    slow_server.join().unwrap();
    fast_server.join().unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline && status.read().unwrap().received_count < 2 {
        std::thread::sleep(Duration::from_millis(10));
    }
    let status = status.read().unwrap();
    assert_eq!(status.received_count, 2);
    assert_eq!(status.forwarded_count, 4);
    assert_eq!(status.suppressed_count, 0);
}

#[test]
fn latest_wins_is_scoped_to_one_device_and_tool() {
    let pending = Arc::new(Mutex::new(HashMap::new()));
    let key = DeliveryKey {
        tool: AiTool::Cursor,
        device_id: "slow-screen".to_owned(),
    };
    let status = Arc::new(RwLock::new(HookRelayStatus {
        pending_count: 2,
        ..HookRelayStatus::default()
    }));
    let mut running = relay("beforeSubmitPrompt", HookBehavior::Running);
    running.tool = AiTool::Cursor;
    let mut asking = relay("beforeShellExecution", HookBehavior::Asking);
    asking.tool = AiTool::Cursor;
    let running_tracker = DeliveryTracker::new(Arc::clone(&status), &running, 1);
    let asking_tracker = DeliveryTracker::new(Arc::clone(&status), &asking, 1);
    pending.lock().unwrap().insert(
        key.clone(),
        VecDeque::from([PendingTargetRelay {
            transition: running.transition,
            tracker: running_tracker,
        }]),
    );
    let (wake_sender, _wake_receiver) = mpsc::sync_channel(1);
    let scheduler = DeliveryScheduler {
        client: reqwest::blocking::Client::new(),
        data: Arc::new(RwLock::new(SavedMonitorData::default())),
        online_devices: Arc::new(RwLock::new(Vec::new())),
        status: Arc::clone(&status),
        pending: Arc::clone(&pending),
        wake_senders: HashMap::from([(key.clone(), wake_sender)]),
    };

    scheduler.enqueue_target(
        &key,
        PendingTargetRelay {
            transition: asking.transition,
            tracker: asking_tracker,
        },
    );

    let pending = pending.lock().unwrap();
    assert_eq!(pending[&key].len(), 1);
    assert_eq!(
        pending[&key].front().unwrap().transition,
        HookTransition::Display(HookBehavior::Asking)
    );
    let status = status.read().unwrap();
    assert_eq!(status.received_count, 1);
    assert_eq!(status.suppressed_count, 1);
    assert_eq!(status.pending_count, 1);
}

#[test]
fn passthrough_tools_keep_every_state_in_each_target_queue() {
    let pending = Arc::new(Mutex::new(HashMap::new()));
    let key = DeliveryKey {
        tool: AiTool::WorkBuddy,
        device_id: "screen-1".to_owned(),
    };
    let status = Arc::new(RwLock::new(HookRelayStatus {
        pending_count: 2,
        ..HookRelayStatus::default()
    }));
    let mut first = relay("PreToolUse", HookBehavior::Running);
    first.tool = AiTool::WorkBuddy;
    let mut second = relay("PermissionRequest", HookBehavior::Asking);
    second.tool = AiTool::WorkBuddy;
    let first_tracker = DeliveryTracker::new(Arc::clone(&status), &first, 1);
    let second_tracker = DeliveryTracker::new(Arc::clone(&status), &second, 1);
    pending.lock().unwrap().insert(
        key.clone(),
        VecDeque::from([PendingTargetRelay {
            transition: first.transition,
            tracker: first_tracker,
        }]),
    );
    let (wake_sender, _wake_receiver) = mpsc::sync_channel(1);
    let scheduler = DeliveryScheduler {
        client: reqwest::blocking::Client::new(),
        data: Arc::new(RwLock::new(SavedMonitorData::default())),
        online_devices: Arc::new(RwLock::new(Vec::new())),
        status: Arc::clone(&status),
        pending: Arc::clone(&pending),
        wake_senders: HashMap::from([(key.clone(), wake_sender)]),
    };

    scheduler.enqueue_target(
        &key,
        PendingTargetRelay {
            transition: second.transition,
            tracker: second_tracker,
        },
    );

    let pending = pending.lock().unwrap();
    assert_eq!(pending[&key].len(), 2);
    assert_eq!(status.read().unwrap().suppressed_count, 0);
}
