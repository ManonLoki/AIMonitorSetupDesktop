// Hook 中继状态的记账工具：把处理结果（成功/失败/抑制/中继层错误）累计
// 写入 `HookRelayStatus`，供前端工作台查询展示。
use std::sync::{Arc, RwLock};

use tracing::{debug, warn};

use crate::{
    application::monitor::{HookRelayLastEvent, HookRelayStatus},
    domain::monitor::{AiTool, HookTransition},
};

/// 记录一次 Hook 事件已处理完成的公共字段，成功/抑制两条路径在此基础上
/// 各自补充结果专属字段，避免重复维护同一份计数逻辑。
fn begin_hook_completion(current: &mut HookRelayStatus) {
    // 收到总数加一。
    current.received_count += 1;
    // 待处理计数减一（不会低于 0）。
    current.pending_count = current.pending_count.saturating_sub(1);
}

// 记录一次真实转发（非抑制）的处理结果。`transition` 为 None
// 表示失败发生在业务转换之前，此时保留已有 last_event，不猜测为 Release。
pub(super) fn record_hook_results(
    status: &Arc<RwLock<HookRelayStatus>>,
    tool: AiTool,
    hook_type: &str,
    transition: Option<HookTransition>,
    forwarded: u64,
    errors: &[String],
) {
    record_hook_results_with_accounting(
        status, tool, hook_type, transition, forwarded, errors, true,
    );
}

// `record_hook_results` 的通用实现：`counts_as_hook` 为 false 时（内部超时
// 释放）跳过收到数/待处理数记账，仍记录显式 transition 及投递结果。
pub(super) fn record_hook_results_with_accounting(
    status: &Arc<RwLock<HookRelayStatus>>,
    tool: AiTool,
    hook_type: &str,
    transition: Option<HookTransition>,
    forwarded: u64,
    errors: &[String],
    counts_as_hook: bool,
) {
    if errors.is_empty() {
        debug!(?tool, hook_type, forwarded, "hook 事件已转发");
    } else {
        warn!(?tool, hook_type, ?errors, "hook 事件转发存在失败");
    }
    if let Ok(mut current) = status.write() {
        if counts_as_hook {
            begin_hook_completion(&mut current);
        }
        current.forwarded_count += forwarded;
        current.failed_count += errors.len() as u64;
        if let Some(transition) = transition {
            current.last_event = Some(match transition {
                HookTransition::Display(behavior) => HookRelayLastEvent::Display {
                    tool,
                    hook_type: hook_type.to_owned(),
                    behavior,
                },
                HookTransition::Release => HookRelayLastEvent::Release {
                    tool,
                    hook_type: hook_type.to_owned(),
                },
            });
        }
        // 多个设备的错误信息用中文顿号拼接展示。
        current.last_error = errors.join("；");
    }
}

// 记录一次被抑制（未真正转发）的 Hook 事件：仍计入收到总数，但归入抑制计数。
pub(super) fn record_suppressed_hook(
    status: &Arc<RwLock<HookRelayStatus>>,
    tool: AiTool,
    hook_type: &str,
) {
    debug!(?tool, hook_type, "hook 事件被抑制");
    if let Ok(mut current) = status.write() {
        begin_hook_completion(&mut current);
        current.suppressed_count += 1;
        current.last_event = Some(HookRelayLastEvent::Suppressed {
            tool,
            hook_type: hook_type.to_owned(),
        });
        current.last_error.clear();
    }
}

// 同一事件已投递到部分目标、但在另一些慢目标的 latest-wins 队列中被更新状态
// 覆盖时，只增加时序抑制计数；事件本身的 received/pending 已由聚合 tracker 记账。
pub(super) fn record_partial_suppression(status: &Arc<RwLock<HookRelayStatus>>) {
    if let Ok(mut current) = status.write() {
        current.suppressed_count += 1;
    }
}

// 记录一次中继层面的失败（如监听启动失败、请求解析失败等，与具体转发无关）。
pub(crate) fn record_relay_failure(status: &Arc<RwLock<HookRelayStatus>>, error: String) {
    warn!(%error, "hook 中继层失败");
    if let Ok(mut current) = status.write() {
        current.failed_count += 1;
        current.last_error = error;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_transition_failure_preserves_last_event_and_records_the_error() {
        let last_event = HookRelayLastEvent::Display {
            tool: AiTool::Codex,
            hook_type: "UserPromptSubmit".to_owned(),
            behavior: crate::domain::monitor::HookBehavior::Running,
        };
        let status = Arc::new(RwLock::new(HookRelayStatus {
            pending_count: 1,
            last_event: Some(last_event.clone()),
            ..HookRelayStatus::default()
        }));

        record_hook_results(
            &status,
            AiTool::Codex,
            "UnknownHook",
            None,
            0,
            &["处理 Hook 前失败".to_owned()],
        );

        let status = status.read().unwrap();
        assert_eq!(status.received_count, 1);
        assert_eq!(status.pending_count, 0);
        assert_eq!(status.failed_count, 1);
        assert_eq!(status.last_event, Some(last_event));
        assert_eq!(status.last_error, "处理 Hook 前失败");
    }

    #[test]
    fn suppressed_event_is_explicit_and_serializes_with_a_stable_kind() {
        let status = Arc::new(RwLock::new(HookRelayStatus {
            pending_count: 1,
            ..HookRelayStatus::default()
        }));

        record_suppressed_hook(&status, AiTool::Codex, "PostToolUse");

        let status = status.read().unwrap();
        assert_eq!(
            status.last_event,
            Some(HookRelayLastEvent::Suppressed {
                tool: AiTool::Codex,
                hook_type: "PostToolUse".to_owned(),
            })
        );
        let json = serde_json::to_value(status.last_event.as_ref().unwrap()).unwrap();
        assert_eq!(json["kind"], "suppressed");
        assert_eq!(json["hookType"], "PostToolUse");
    }
}
