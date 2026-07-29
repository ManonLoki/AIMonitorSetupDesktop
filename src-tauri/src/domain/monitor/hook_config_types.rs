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
    pub hermes: String,
    #[serde(default)]
    pub open_claw: String,
    #[serde(default)]
    pub code_buddy: String,
    #[serde(default)]
    pub qwen_code: String,
    #[serde(default)]
    pub kimi_code: String,
    #[serde(default)]
    pub qoder: String,
    #[serde(default)]
    pub gemini_cli: String,
    #[serde(default)]
    pub github_copilot: String,
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
            AiTool::QwenCode => &self.qwen_code,
            AiTool::KimiCode => &self.kimi_code,
            AiTool::Qoder => &self.qoder,
            AiTool::GeminiCli => &self.gemini_cli,
            AiTool::GitHubCopilot => &self.github_copilot,
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
            AiTool::QwenCode => self.qwen_code = directory,
            AiTool::KimiCode => self.kimi_code = directory,
            AiTool::Qoder => self.qoder = directory,
            AiTool::GeminiCli => self.gemini_cli = directory,
            AiTool::GitHubCopilot => self.github_copilot = directory,
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
    fn hook_config_uses_only_canonical_hermes_names() {
        assert!(serde_json::from_str::<AiTool>(r#""harness""#).is_err());
        let tool: AiTool = serde_json::from_str(r#""hermes""#).unwrap();
        assert_eq!(tool, AiTool::Hermes);
        assert_eq!(serde_json::to_string(&tool).unwrap(), r#""hermes""#);

        let directories: HookConfigDirectories =
            serde_json::from_str(r#"{"hermes":"/hooks/hermes"}"#).unwrap();
        assert_eq!(directories.hermes, "/hooks/hermes");
        let serialized = serde_json::to_value(directories).unwrap();
        assert_eq!(serialized["hermes"], "/hooks/hermes");
        assert!(serialized.get("harness").is_none());
    }

    #[test]
    fn hook_config_directories_cover_new_phase2_tools() {
        let mut directories = HookConfigDirectories::default();
        directories.set(AiTool::QwenCode, "/qwen".to_owned());
        directories.set(AiTool::KimiCode, "/kimi-code".to_owned());
        directories.set(AiTool::Qoder, "/qoder".to_owned());
        directories.set(AiTool::GeminiCli, "/gemini".to_owned());
        directories.set(AiTool::GitHubCopilot, "/copilot".to_owned());

        assert_eq!(directories.get(AiTool::QwenCode), "/qwen");
        assert_eq!(directories.get(AiTool::KimiCode), "/kimi-code");
        assert_eq!(directories.get(AiTool::Qoder), "/qoder");
        assert_eq!(directories.get(AiTool::GeminiCli), "/gemini");
        assert_eq!(directories.get(AiTool::GitHubCopilot), "/copilot");

        let serialized = serde_json::to_value(directories).unwrap();
        assert_eq!(serialized["qwenCode"], "/qwen");
        assert_eq!(serialized["kimiCode"], "/kimi-code");
        assert_eq!(serialized["qoder"], "/qoder");
        assert_eq!(serialized["geminiCli"], "/gemini");
        assert_eq!(serialized["githubCopilot"], "/copilot");
    }
}
