// Hook 中继状态的记账工具：把处理结果（成功/失败/抑制/中继层错误）累计
// 写入 `HookRelayStatus`，供前端工作台查询展示。
use std::sync::{Arc, RwLock};

use crate::{
    application::monitor::HookRelayStatus,
    domain::monitor::{AiTool, HookBehavior},
};

/// 记录一次 Hook 事件已处理完成的公共字段，成功/抑制两条路径在此基础上
/// 各自补充结果专属字段，避免重复维护同一份计数逻辑。
fn begin_hook_completion(current: &mut HookRelayStatus, tool: AiTool, hook_type: &str) {
    // 收到总数加一。
    current.received_count += 1;
    // 待处理计数减一（不会低于 0）。
    current.pending_count = current.pending_count.saturating_sub(1);
    // 记录最近一次涉及的工具和事件类型。
    current.last_tool = Some(tool);
    hook_type.clone_into(&mut current.last_hook_type);
}

// 记录一次真实转发（非抑制）的处理结果：成功次数、失败次数、最近行为与错误信息。
pub(super) fn record_hook_results(
    status: &Arc<RwLock<HookRelayStatus>>,
    tool: AiTool,
    hook_type: &str,
    behavior: Option<HookBehavior>,
    forwarded: u64,
    errors: &[String],
) {
    record_hook_results_with_accounting(status, tool, hook_type, behavior, forwarded, errors, true);
}

// `record_hook_results` 的通用实现：`counts_as_hook` 为 false 时（内部超时
// 释放）跳过收到数/待处理数的记账，只更新最近工具/类型/转发结果。
pub(super) fn record_hook_results_with_accounting(
    status: &Arc<RwLock<HookRelayStatus>>,
    tool: AiTool,
    hook_type: &str,
    behavior: Option<HookBehavior>,
    forwarded: u64,
    errors: &[String],
    counts_as_hook: bool,
) {
    if let Ok(mut current) = status.write() {
        if counts_as_hook {
            begin_hook_completion(&mut current, tool, hook_type);
        } else {
            // 超时清扫产生的是内部状态转换，不凭空增加收到数，也不消耗一个
            // pending；仍更新最近工具/类型，让自动释放在工作台中可解释。
            current.last_tool = Some(tool);
            hook_type.clone_into(&mut current.last_hook_type);
        }
        current.forwarded_count += forwarded;
        current.failed_count += errors.len() as u64;
        current.last_behavior = behavior;
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
    if let Ok(mut current) = status.write() {
        begin_hook_completion(&mut current, tool, hook_type);
        current.suppressed_count += 1;
        // 忽略事件不会改变已经转发到设备的最后行为。
        current.last_error.clear();
    }
}

// 记录一次中继层面的失败（如监听启动失败、请求解析失败等，与具体转发无关）。
pub(crate) fn record_relay_failure(status: &Arc<RwLock<HookRelayStatus>>, error: String) {
    if let Ok(mut current) = status.write() {
        current.failed_count += 1;
        current.last_error = error;
    }
}
