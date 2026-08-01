use std::{
    io::{Read, Write},
    net::{Ipv4Addr, TcpListener},
    sync::mpsc,
    time::{Duration, Instant},
};

use super::*;
use crate::{
    application::monitor::{DEFAULT_DEVICE_API_PATH, test_support::test_profile},
    domain::monitor::{
        DiscoverySource, HookBehavior, HookConfigDirectories, MonitorDeviceRoute, MonitorSettings,
    },
};

fn cursor_target(
    address: std::net::SocketAddr,
) -> (
    Arc<RwLock<SavedMonitorData>>,
    Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
) {
    let mut profile = test_profile();
    profile.tool = AiTool::Cursor;
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
    let online = Arc::new(RwLock::new(vec![DiscoveredMonitorDevice {
        id: "screen-1".to_owned(),
        name: "Desk".to_owned(),
        api_version: "1".to_owned(),
        base_url: format!("http://{address}"),
        path: DEFAULT_DEVICE_API_PATH.to_owned(),
        discovery_source: DiscoverySource::Mdns,
    }]));
    (data, online)
}

fn pending_relay(hook_type: &str, transition: HookTransition) -> PendingHookRelay {
    PendingHookRelay {
        tool: AiTool::Cursor,
        hook_type: hook_type.to_owned(),
        transition,
        counts_as_hook: true,
    }
}

fn tracked_target(
    relay: &PendingHookRelay,
    status: &Arc<RwLock<HookRelayStatus>>,
) -> PendingTargetRelay {
    PendingTargetRelay {
        transition: relay.transition,
        tracker: DeliveryTracker::new(Arc::clone(status), relay, 1),
    }
}

fn receive_http_request(listener: &TcpListener, timeout: Duration) -> Option<String> {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 2048];
                loop {
                    let Ok(length) = stream.read(&mut buffer) else {
                        return None;
                    };
                    if length == 0 {
                        return None;
                    }
                    request.extend_from_slice(&buffer[..length]);
                    let Some(header_end) = request
                        .windows(4)
                        .position(|bytes| bytes == b"\r\n\r\n")
                        .map(|position| position + 4)
                    else {
                        continue;
                    };
                    let headers = std::str::from_utf8(&request[..header_end]).unwrap();
                    let content_length = headers
                        .split("\r\n")
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= header_end + content_length {
                        break;
                    }
                }
                let request = String::from_utf8(request).unwrap();
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .unwrap();
                return Some(request);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("test HTTP listener failed: {error}"),
        }
    }
}

fn wait_for_hook_completion(
    status: &Arc<RwLock<HookRelayStatus>>,
    timeout: Duration,
) -> HookRelayStatus {
    let deadline = Instant::now() + timeout;
    loop {
        let snapshot = status.read().unwrap().clone();
        if snapshot.pending_count == 0 {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "Hook tracker did not finish before timeout: {snapshot:?}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn wait_until(timeout: Duration, message: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !condition() {
        assert!(Instant::now() < deadline, "{message}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn cursor_display_within_handoff_grace_suppresses_staged_release() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server =
        std::thread::spawn(move || receive_http_request(&listener, Duration::from_secs(15)));
    let (data, online) = cursor_target(address);
    let status = Arc::new(RwLock::new(HookRelayStatus {
        pending_count: 2,
        ..HookRelayStatus::default()
    }));
    let pending = Arc::new(Mutex::new(HashMap::new()));
    let key = DeliveryKey {
        tool: AiTool::Cursor,
        device_id: "screen-1".to_owned(),
    };
    let (wake_sender, wake_receiver) = mpsc::sync_channel(1);
    spawn_target_worker(
        key.clone(),
        reqwest::blocking::Client::new(),
        wake_receiver,
        Arc::clone(&pending),
        Arc::clone(&data),
        Arc::clone(&online),
        Duration::from_secs(2),
    );
    let release = pending_relay("sessionEnd", HookTransition::Release);
    pending.lock().unwrap().insert(
        key.clone(),
        VecDeque::from([tracked_target(&release, &status)]),
    );
    wake_sender.send(()).unwrap();
    wait_until(
        Duration::from_secs(1),
        "target worker did not stage the release before timeout",
        || !pending.lock().unwrap().contains_key(&key),
    );
    let scheduler = DeliveryScheduler {
        client: reqwest::blocking::Client::new(),
        data,
        online_devices: online,
        status: Arc::clone(&status),
        pending,
        wake_senders: HashMap::from([(key.clone(), wake_sender)]),
    };
    let idle = pending_relay("sessionStart", HookTransition::Display(HookBehavior::Idle));
    scheduler.enqueue_target(&key, tracked_target(&idle, &status));

    let request = server
        .join()
        .unwrap()
        .unwrap_or_else(|| panic!("no handoff request; status={:?}", status.read().unwrap()));
    assert!(request.starts_with("POST /api/slots/1 HTTP/1.1"));
    assert!(request.contains(r#""behavior":"idle""#));
    let status = wait_for_hook_completion(&status, Duration::from_secs(2));
    assert_eq!(status.received_count, 2);
    assert_eq!(status.forwarded_count, 1);
    assert_eq!(status.suppressed_count, 1);
    assert_eq!(status.pending_count, 0);
}

#[test]
fn release_grace_expires_without_a_successor() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let first = receive_http_request(&listener, Duration::from_secs(15));
        let second = receive_http_request(&listener, Duration::from_millis(350));
        (first, second)
    });
    let (data, online) = cursor_target(address);
    let status = Arc::new(RwLock::new(HookRelayStatus {
        pending_count: 1,
        ..HookRelayStatus::default()
    }));
    let mut scheduler = DeliveryScheduler::new(
        &reqwest::blocking::Client::new(),
        &data,
        &online,
        Arc::clone(&status),
    );

    scheduler.enqueue(&pending_relay("sessionEnd", HookTransition::Release));

    let (first, second) = server.join().unwrap();
    let first =
        first.unwrap_or_else(|| panic!("no release request; status={:?}", status.read().unwrap()));
    assert!(first.starts_with("DELETE /api/slots/1 HTTP/1.1"));
    assert!(second.is_none());
    let status = wait_for_hook_completion(&status, Duration::from_secs(2));
    assert_eq!(status.received_count, 1);
    assert_eq!(status.forwarded_count, 1);
    assert_eq!(status.suppressed_count, 0);
    assert_eq!(status.pending_count, 0);
}

#[test]
fn stale_wake_token_does_not_suppress_release_without_pending_state() {
    let pending = Arc::new(Mutex::new(HashMap::new()));
    let key = DeliveryKey {
        tool: AiTool::Cursor,
        device_id: "screen-1".to_owned(),
    };
    let (wake_sender, wake_receiver) = mpsc::sync_channel(1);
    wake_sender.send(()).unwrap();

    assert!(
        !release_has_pending_successor(&wake_receiver, &pending, &key, Duration::from_millis(5),)
            .unwrap()
    );
}

#[test]
fn poisoned_pending_queue_fails_all_trackers_and_stops_the_worker() {
    let pending = Arc::new(Mutex::new(HashMap::new()));
    let key = DeliveryKey {
        tool: AiTool::Cursor,
        device_id: "screen-1".to_owned(),
    };
    let status = Arc::new(RwLock::new(HookRelayStatus {
        pending_count: 2,
        ..HookRelayStatus::default()
    }));
    let release = pending_relay("sessionEnd", HookTransition::Release);
    let idle = pending_relay("sessionStart", HookTransition::Display(HookBehavior::Idle));
    pending.lock().unwrap().insert(
        key.clone(),
        VecDeque::from([
            tracked_target(&release, &status),
            tracked_target(&idle, &status),
        ]),
    );
    let pending_to_poison = Arc::clone(&pending);
    assert!(
        std::thread::spawn(move || {
            let _guard = pending_to_poison.lock().unwrap();
            panic!("poison target queue");
        })
        .join()
        .is_err()
    );
    let (wake_sender, wake_receiver) = mpsc::sync_channel(1);
    spawn_target_worker(
        key.clone(),
        reqwest::blocking::Client::new(),
        wake_receiver,
        Arc::clone(&pending),
        Arc::new(RwLock::new(SavedMonitorData::default())),
        Arc::new(RwLock::new(Vec::new())),
        Duration::from_millis(5),
    );
    wake_sender.send(()).unwrap();
    let status = wait_for_hook_completion(&status, Duration::from_secs(2));
    wait_until(
        Duration::from_secs(2),
        "target worker did not exit after the pending queue was poisoned",
        || {
            matches!(
                wake_sender.try_send(()),
                Err(mpsc::TrySendError::Disconnected(()))
            )
        },
    );
    assert_eq!(status.received_count, 2);
    assert_eq!(status.failed_count, 2);
    assert_eq!(status.forwarded_count, 0);
    assert_eq!(status.suppressed_count, 0);
    assert_eq!(status.pending_count, 0);
    assert_eq!(
        status.last_error,
        "Hook 目标队列不可用，目标投递 worker 已停止"
    );
    match pending.lock() {
        Ok(_) => panic!("pending queue should remain poisoned for later enqueues to fail closed"),
        Err(poisoned) => assert!(!poisoned.into_inner().contains_key(&key)),
    }
}

#[test]
fn full_wake_channel_still_drives_the_worker_to_complete_the_pending_transition() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let (data, online) = cursor_target(address);
    let pending = Arc::new(Mutex::new(HashMap::new()));
    let key = DeliveryKey {
        tool: AiTool::Cursor,
        device_id: "screen-1".to_owned(),
    };
    let status = Arc::new(RwLock::new(HookRelayStatus {
        pending_count: 1,
        ..HookRelayStatus::default()
    }));
    let relay = pending_relay("sessionStart", HookTransition::Display(HookBehavior::Idle));
    let target = tracked_target(&relay, &status);
    let (wake_sender, wake_receiver) = mpsc::sync_channel(1);
    wake_sender.send(()).unwrap();
    let scheduler = DeliveryScheduler {
        client: reqwest::blocking::Client::new(),
        data: Arc::clone(&data),
        online_devices: Arc::clone(&online),
        status: Arc::clone(&status),
        pending: Arc::clone(&pending),
        wake_senders: HashMap::from([(key.clone(), wake_sender)]),
    };
    let (finished_sender, finished_receiver) = mpsc::channel();
    let enqueue_key = key.clone();
    let handle = std::thread::spawn(move || {
        scheduler.enqueue_target(&enqueue_key, target);
        finished_sender.send(()).unwrap();
    });

    if finished_receiver
        .recv_timeout(Duration::from_secs(1))
        .is_err()
    {
        drop(wake_receiver);
        handle.join().unwrap();
        panic!("enqueue_target blocked while the wake channel was full");
    }
    handle.join().unwrap();
    assert_eq!(pending.lock().unwrap().values().next().unwrap().len(), 1);

    let server =
        std::thread::spawn(move || receive_http_request(&listener, Duration::from_secs(15)));
    spawn_target_worker(
        key.clone(),
        reqwest::blocking::Client::new(),
        wake_receiver,
        Arc::clone(&pending),
        data,
        online,
        Duration::ZERO,
    );

    let request = server
        .join()
        .unwrap()
        .unwrap_or_else(|| panic!("full wake token never drove the pending transition"));
    assert!(request.starts_with("POST /api/slots/1 HTTP/1.1"));
    assert!(request.contains(r#""behavior":"idle""#));
    let status = wait_for_hook_completion(&status, Duration::from_secs(2));
    assert_eq!(status.received_count, 1);
    assert_eq!(status.forwarded_count, 1);
    assert_eq!(status.failed_count, 0);
    assert_eq!(status.suppressed_count, 0);
    assert_eq!(status.pending_count, 0);
    assert!(!pending.lock().unwrap().contains_key(&key));
}
