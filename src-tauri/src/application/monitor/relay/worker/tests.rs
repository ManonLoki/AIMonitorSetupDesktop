use super::super::status::record_hook_results_with_accounting;
use super::*;
use crate::domain::monitor::HookBehavior;
use std::{thread, time::Duration};

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
    spawn_hook_worker(
        &reqwest::blocking::Client::new(),
        receiver,
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
