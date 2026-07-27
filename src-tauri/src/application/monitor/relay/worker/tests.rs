use std::{
    io::Write,
    net::{Ipv4Addr, TcpListener},
    time::Instant,
};

use super::*;
use crate::{
    application::monitor::test_support::{read_test_http_request, two_tool_delivery_data},
    domain::monitor::{HookBehavior, HookTransition},
};

use super::super::status::record_hook_results_with_accounting;

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
    let (wake_sender, _wake_receiver) = mpsc::sync_channel::<()>(HOOK_RELAY_WAKE_QUEUE_CAPACITY);
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
    let (wake_sender, _wake_receiver) = mpsc::sync_channel::<()>(HOOK_RELAY_WAKE_QUEUE_CAPACITY);
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
