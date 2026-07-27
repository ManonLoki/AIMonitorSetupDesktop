// serde：结构体的序列化与反序列化派生宏。
use serde::{Deserialize, Serialize};

use super::device::AiTool;

// 生成的 Hooks 配置文件预览：目标文件名 + 文件内容（尚未写入磁盘）。
#[derive(Clone, Debug)]
pub struct HookConfigPreview {
    pub filename: String,
    pub content: String,
}

// 一次写入 Hooks 配置到磁盘后的结果，返回给前端展示。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookConfigWriteResult {
    pub tool: AiTool,
    pub filename: String,
    pub config_changed: bool,
    /// 工具要求用户审核新 Hook 且配置发生变化时为真。
    pub requires_review: bool,
    /// 工具需要重启当前会话或守护进程才能加载新配置时为真。
    pub restart_required: bool,
}

// 用户为各工具自定义的 Hooks 配置目录（为空表示使用默认目录）。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookConfigDirectories {
    #[serde(default)]
    pub codex: String,
    #[serde(default)]
    pub claude_code: String,
    #[serde(default)]
    pub cursor: String,
    #[serde(default)]
    pub open_code: String,
    #[serde(default)]
    pub work_buddy: String,
    #[serde(default)]
    #[serde(alias = "harness")]
    pub hermes: String,
    #[serde(default)]
    pub open_claw: String,
    #[serde(default)]
    pub code_buddy: String,
}

impl HookConfigDirectories {
    // 按工具类型取出对应的自定义目录（可能为空字符串）。
    pub fn get(&self, tool: AiTool) -> &str {
        match tool {
            AiTool::Codex => &self.codex,
            AiTool::ClaudeCode => &self.claude_code,
            AiTool::Cursor => &self.cursor,
            AiTool::OpenCode => &self.open_code,
            AiTool::WorkBuddy => &self.work_buddy,
            AiTool::Hermes => &self.hermes,
            AiTool::OpenClaw => &self.open_claw,
            AiTool::CodeBuddy => &self.code_buddy,
        }
    }

    // 按工具类型写入对应的自定义目录。
    pub fn set(&mut self, tool: AiTool, directory: String) {
        match tool {
            AiTool::Codex => self.codex = directory,
            AiTool::ClaudeCode => self.claude_code = directory,
            AiTool::Cursor => self.cursor = directory,
            AiTool::OpenCode => self.open_code = directory,
            AiTool::WorkBuddy => self.work_buddy = directory,
            AiTool::Hermes => self.hermes = directory,
            AiTool::OpenClaw => self.open_claw = directory,
            AiTool::CodeBuddy => self.code_buddy = directory,
        }
    }
}

// 某工具 Hooks 配置文件的最终定位信息（目录 + 完整路径 + 是否自定义目录）。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookConfigLocation {
    pub tool: AiTool,
    pub directory: String,
    pub config_path: String,
    pub is_custom: bool,
}

// 仅在测试构建中编译的单元测试模块，覆盖本文件内的纯业务逻辑。
#[cfg(test)]
mod tests {
    use super::{AiTool, HookConfigDirectories};

    #[test]
    fn legacy_harness_values_load_as_hermes_and_serialize_canonically() {
        let tool: AiTool = serde_json::from_str(r#""harness""#).unwrap();
        assert_eq!(tool, AiTool::Hermes);
        assert_eq!(serde_json::to_string(&tool).unwrap(), r#""hermes""#);

        let directories: HookConfigDirectories =
            serde_json::from_str(r#"{"harness":"/legacy/harness"}"#).unwrap();
        assert_eq!(directories.hermes, "/legacy/harness");
        let serialized = serde_json::to_value(directories).unwrap();
        assert_eq!(serialized["hermes"], "/legacy/harness");
        assert!(serialized.get("harness").is_none());
    }
}
