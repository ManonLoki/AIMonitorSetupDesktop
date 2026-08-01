use serde::{Deserialize, Serialize};

use super::{
    DEFAULT_PROFILE_SLOT,
    device::{AiProfile, AiTool, HookBehavior, HookContent},
};

/// 前端编辑一个 AI 工具展示配置时使用的传输草稿。
///
/// 设备归属不是可编辑字段；保存时由 application 层绑定当前选中的设备，避免
/// 调用方把草稿写入其他设备。未保存的工具也由 Rust 生成完整的四行为草稿。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiProfileDraft {
    pub tool: AiTool,
    pub slot: u8,
    #[serde(default)]
    pub hooks: Vec<HookContent>,
}

/// 同一次读取产生的设备并发令牌与完整 Profile 草稿集合。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiProfileDraftSet {
    pub expected_device_id: String,
    pub drafts: Vec<AiProfileDraft>,
}

impl AiProfileDraft {
    /// 按领域层声明的默认槽位和固定行为顺序生成一个可编辑的空白草稿。
    pub fn default_for(tool: AiTool) -> Self {
        Self {
            tool,
            slot: DEFAULT_PROFILE_SLOT,
            hooks: HookBehavior::DISPLAY_BEHAVIORS
                .into_iter()
                .map(|behavior| HookContent {
                    behavior,
                    content: String::new(),
                    image: String::new(),
                })
                .collect(),
        }
    }

    /// 把无设备归属的传输草稿转换为内部持久化实体。
    pub(crate) fn bind_to_device(self, device_id: &str) -> AiProfile {
        AiProfile {
            device_id: device_id.to_owned(),
            tool: self.tool,
            slot: self.slot,
            hooks: self.hooks,
        }
    }
}

impl From<&AiProfile> for AiProfileDraft {
    fn from(profile: &AiProfile) -> Self {
        Self {
            tool: profile.tool,
            slot: profile.slot,
            hooks: profile.hooks.clone(),
        }
    }
}

impl From<AiProfile> for AiProfileDraft {
    fn from(profile: AiProfile) -> Self {
        Self {
            tool: profile.tool,
            slot: profile.slot,
            hooks: profile.hooks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_draft_uses_the_domain_slot_and_behavior_order() {
        let draft = AiProfileDraft::default_for(AiTool::Codex);

        assert_eq!(draft.slot, DEFAULT_PROFILE_SLOT);
        assert_eq!(
            draft
                .hooks
                .iter()
                .map(|hook| hook.behavior)
                .collect::<Vec<_>>(),
            HookBehavior::DISPLAY_BEHAVIORS
        );
        assert!(
            draft
                .hooks
                .iter()
                .all(|hook| hook.content.is_empty() && hook.image.is_empty())
        );
    }

    #[test]
    fn serialized_draft_does_not_expose_device_ownership() {
        let value = serde_json::to_value(AiProfileDraft::default_for(AiTool::Cursor)).unwrap();

        assert!(value.get("deviceId").is_none());
        assert_eq!(value.get("tool").unwrap(), "cursor");
    }

    #[test]
    fn draft_set_serializes_the_device_token_in_camel_case() {
        let value = serde_json::to_value(AiProfileDraftSet {
            expected_device_id: "screen-1".to_owned(),
            drafts: vec![AiProfileDraft::default_for(AiTool::Codex)],
        })
        .unwrap();

        assert_eq!(value.get("expectedDeviceId").unwrap(), "screen-1");
        assert!(value.get("expected_device_id").is_none());
    }
}
