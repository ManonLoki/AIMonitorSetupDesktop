use std::{sync::mpsc, time::Instant};

use super::{DeliveryKey, PendingTargetRelays};

// Cursor 等声明了交接缓冲的协议，在真正发送 destructive DELETE 前等待同一
// “设备 + 工具”的继任展示。唤醒令牌可能是 worker 忙于上一请求时留下的旧令牌，
// 因此必须以 pending 队列为准，并持续排空旧令牌直到 deadline。
pub(super) fn release_has_pending_successor(
    receiver: &mpsc::Receiver<()>,
    pending: &PendingTargetRelays,
    key: &DeliveryKey,
    settle_delay: std::time::Duration,
) -> Result<bool, String> {
    if settle_delay.is_zero() {
        return Ok(false);
    }
    let deadline = Instant::now() + settle_delay;
    loop {
        if pending_has_successor(pending, key)? {
            return Ok(true);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        match receiver.recv_timeout(remaining) {
            Ok(()) => {}
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {
                return pending_has_successor(pending, key);
            }
        }
    }
}

fn pending_has_successor(pending: &PendingTargetRelays, key: &DeliveryKey) -> Result<bool, String> {
    let pending = pending
        .lock()
        .map_err(|_| "Hook 目标队列不可用，已拒绝发送释放请求".to_owned())?;
    Ok(pending.get(key).is_some_and(|queue| !queue.is_empty()))
}
