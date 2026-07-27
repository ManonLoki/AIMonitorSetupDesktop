// serde：结构体/枚举的序列化与反序列化派生宏。
use serde::{Deserialize, Serialize};

// 一条已持久化的设备路由：设备 ID/名称 + 其基地址，用于 Hook 转发目标定位。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorDeviceRoute {
    pub base_url: String,
    pub device_id: String,
    pub device_name: String,
}

// 一次发现流程中找到的设备原始信息（尚未落库为路由）。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredMonitorDevice {
    pub id: String,
    pub name: String,
    pub api_version: String,
    pub base_url: String,
    pub path: String,
    // 缺失时默认视为通过 mDNS 发现。
    #[serde(default)]
    pub discovery_source: DiscoverySource,
}

/// 设备是如何被找到的；决定发现流程的信任优先级：mDNS 优先，
/// 失败后回退到 UDP 广播，再回退到已保存地址。
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DiscoverySource {
    // 默认来源：mDNS 局域网发现，信任优先级最高。
    #[default]
    Mdns,
    UdpBroadcast,
    SavedAddress,
}

// 应用支持接入的 AI 工具；Hash 派生用于放入 HashSet 做去重校验。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum AiTool {
    Codex,
    ClaudeCode,
    Cursor,
    OpenCode,
    WorkBuddy,
    #[serde(alias = "harness")]
    Hermes,
    OpenClaw,
    CodeBuddy,
}

impl AiTool {
    // 遍历全部工具时使用的固定顺序数组。
    pub const ALL: [Self; 8] = [
        Self::Codex,
        Self::ClaudeCode,
        Self::Cursor,
        Self::OpenCode,
        Self::WorkBuddy,
        Self::Hermes,
        Self::OpenClaw,
        Self::CodeBuddy,
    ];
}

/// 按应用固定顺序规范化用户选择，并消除重复项。
pub fn normalize_enabled_ai_tools(selected: &[AiTool]) -> Vec<AiTool> {
    AiTool::ALL
        .into_iter()
        .filter(|tool| selected.contains(tool))
        .collect()
}

/// AI 实例在展示屏上呈现的状态。`Idle`/`Running`/`Asking`/`Error` 是当前
/// 有效的四种展示行为（见 `DISPLAY_BEHAVIORS`），每个 Profile 必须四选四配齐。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum HookBehavior {
    Idle,
    Running,
    Asking,
    Error,
}

impl HookBehavior {
    // 当前所有有效的展示行为，顺序固定，供 validate_profile 做“四选四”检查。
    pub(super) const DISPLAY_BEHAVIORS: [Self; 4] =
        [Self::Idle, Self::Running, Self::Asking, Self::Error];
}

// 某个展示行为对应的具体内容：文案 + 图片文件名。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HookContent {
    pub behavior: HookBehavior,
    // 文案允许为空，缺省填空字符串。
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub image: String,
}

// 一个 AI 工具在某设备、某展示位上的完整配置。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProfile {
    /// Profile 所属的 `AIMonitor` 设备。
    #[serde(default)]
    pub device_id: String,
    pub tool: AiTool,
    /// 在展示屏上的显示位置，取值范围 1-25（校验见 `validate_profile`）。
    pub slot: u8,
    // 四种行为的内容列表；数量与内容由 validate_profile 校验。
    #[serde(default)]
    pub hooks: Vec<HookContent>,
}

// 仅在测试构建中编译的单元测试模块，覆盖本文件内的纯业务逻辑。
#[cfg(test)]
mod tests {
    use super::{AiTool, normalize_enabled_ai_tools};
    use crate::domain::monitor::settings::default_enabled_ai_tools;

    #[test]
    fn ai_client_selection_defaults_to_primary_tools_and_is_normalized() {
        assert_eq!(
            default_enabled_ai_tools(),
            vec![AiTool::Codex, AiTool::ClaudeCode, AiTool::Cursor]
        );
        assert_eq!(
            normalize_enabled_ai_tools(&[
                AiTool::Cursor,
                AiTool::Codex,
                AiTool::Cursor,
                AiTool::OpenClaw,
            ]),
            vec![AiTool::Codex, AiTool::Cursor, AiTool::OpenClaw]
        );
    }
}
