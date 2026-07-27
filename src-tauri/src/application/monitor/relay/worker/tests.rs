use super::super::status::record_hook_results_with_accounting;
use super::*;

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
