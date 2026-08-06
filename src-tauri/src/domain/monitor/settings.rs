// serde：结构体的序列化与反序列化派生宏。
use serde::{Deserialize, Serialize};

use crate::domain::AppError;

use super::device::AiTool;
use super::{
    DEFAULT_BASE_URL, DEFAULT_DISCOVERY_INTERVAL_MINUTES, MAX_DISCOVERY_INTERVAL_MINUTES,
    MIN_DISCOVERY_INTERVAL_MINUTES,
};

// 与前端 TypeScript 对接的 DTO：序列化为 camelCase 字段名。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorSettings {
    /// 所有设备共享的显示用户名。
    #[serde(default)]
    pub username: String,
    /// 当前 UI 选中的设备。仅用于页面上下文，不决定 Hook 转发目标。
    pub base_url: String,
    // 字段缺失时反序列化为空字符串，保持向后兼容。
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub device_name: String,
    /// 在线设备自动检查间隔（分钟）。修改后由后台发现循环下一次轮询立即生效。
    #[serde(default = "default_discovery_interval_minutes")]
    pub discovery_interval_minutes: u64,
    /// 设置页选中的 AI 客户端；监控管理与 Hooks 管理共用这份可见范围。
    #[serde(default = "default_enabled_ai_tools")]
    pub enabled_ai_tools: Vec<AiTool>,
}

// 供 serde default 属性调用，反序列化时缺省该字段则填入默认间隔。
fn default_discovery_interval_minutes() -> u64 {
    DEFAULT_DISCOVERY_INTERVAL_MINUTES
}

/// 首次启动时默认展示的三个 AI 客户端。
pub fn default_enabled_ai_tools() -> Vec<AiTool> {
    vec![AiTool::Codex, AiTool::ClaudeCode, AiTool::Cursor]
}

// 手动实现 Default，为各字段指定初始值（而不是全部用类型默认值）。
impl Default for MonitorSettings {
    fn default() -> Self {
        Self {
            username: String::new(),
            base_url: DEFAULT_BASE_URL.to_owned(),
            device_id: String::new(),
            device_name: String::new(),
            discovery_interval_minutes: DEFAULT_DISCOVERY_INTERVAL_MINUTES,
            enabled_ai_tools: default_enabled_ai_tools(),
        }
    }
}

/// 校验用户在设置页填写的自动检查间隔，防止 0（忙轮询）或过大的值
/// （长时间感知不到设备上下线）。
pub fn validate_discovery_interval_minutes(minutes: u64) -> Result<u64, AppError> {
    // 超出 [MIN, MAX] 区间则拒绝并返回结构化错误码。
    if !(MIN_DISCOVERY_INTERVAL_MINUTES..=MAX_DISCOVERY_INTERVAL_MINUTES).contains(&minutes) {
        return Err(AppError::new("error.settings.discoveryIntervalOutOfRange")
            .param("min", MIN_DISCOVERY_INTERVAL_MINUTES.to_string())
            .param("max", MAX_DISCOVERY_INTERVAL_MINUTES.to_string()));
    }
    // 校验通过，原样返回该分钟数。
    Ok(minutes)
}

// 仅在测试构建中编译的单元测试模块，覆盖本文件内的纯业务逻辑。
#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_DISCOVERY_INTERVAL_MINUTES, MAX_DISCOVERY_INTERVAL_MINUTES, MonitorSettings,
        validate_discovery_interval_minutes,
    };

    // 验证自动检查间隔的默认值为 1 分钟，且 0 和超过上限的值都会被拒绝，
    // 默认值本身则应该通过校验。
    #[test]
    fn discovery_interval_defaults_to_one_minute_and_rejects_out_of_range_values() {
        assert_eq!(
            MonitorSettings::default().discovery_interval_minutes,
            DEFAULT_DISCOVERY_INTERVAL_MINUTES
        );
        assert_eq!(DEFAULT_DISCOVERY_INTERVAL_MINUTES, 1);
        // 0 属于忙轮询，应当被拒绝。
        assert!(validate_discovery_interval_minutes(0).is_err());
        // 超过最大值同样应当被拒绝。
        assert!(validate_discovery_interval_minutes(MAX_DISCOVERY_INTERVAL_MINUTES + 1).is_err());
        // 默认值本身应当合法。
        assert_eq!(
            validate_discovery_interval_minutes(DEFAULT_DISCOVERY_INTERVAL_MINUTES).unwrap(),
            DEFAULT_DISCOVERY_INTERVAL_MINUTES
        );
    }
}
