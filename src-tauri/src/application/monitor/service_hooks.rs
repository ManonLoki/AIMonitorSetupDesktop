// 已启用 AI 客户端的 Hooks 自动补写编排。
//
// 自动流程先检查当前管理标识：已有时保持原样，缺失时才复用显式写入的
// 完整生成与合并逻辑。单个客户端失败不阻止启动、AI 选项保存或后续处理。

use std::{path::Path, thread};

use tracing::{error, warn};

use super::{MonitorService, config_io::read_optional_config, wsl::WslDirectory};
use crate::domain::AppError;
use crate::domain::monitor::{
    AiTool, ai_tool_name, hook_config_filename, hook_config_has_managed_marker,
};

impl MonitorService {
    /// 在独立线程中执行自动补写，避免本地磁盘或 WSL 配置访问阻塞应用启动与
    /// “保存 AI 客户端”命令。线程只持有共享服务句柄，失败仍由同步实现逐项记录。
    pub(crate) fn start_auto_write_enabled_hook_configs(&self) {
        let service = self.clone();
        if let Err(error) = thread::Builder::new()
            .name("aimonitor-auto-hooks".to_owned())
            .spawn(move || service.auto_write_enabled_hook_configs())
        {
            error!(%error, "无法启动 Hooks 自动补写任务");
        }
    }

    /// 为当前已启用的 AI 客户端逐个补写 Hooks。
    ///
    /// 已包含 `AIMonitor` 管理标识的配置保持原样；没有标识时才尝试写入。
    /// 错误采用 best-effort 语义：
    /// 配置选择本身仍然有效，用户可随后在“Hooks 管理”中修正路径或手动重写。
    fn auto_write_enabled_hook_configs(&self) {
        let tools = match self.settings() {
            Ok(settings) => settings.enabled_ai_tools,
            Err(error) => {
                error!(%error, "无法读取自动 Hooks 写入范围");
                return;
            }
        };

        for tool in tools {
            let result = self.write_hook_config_automatically(tool);
            if let Err(error) = result {
                warn!(tool = ai_tool_name(tool), %error, "无法自动写入 Hooks");
            }
        }
    }

    fn write_hook_config_automatically(&self, tool: AiTool) -> Result<(), AppError> {
        let location = self
            .hook_config_locations()?
            .into_iter()
            .find(|location| location.tool == tool)
            .ok_or_else(|| {
                AppError::new("error.hooks.locationNotFound").param("tool", ai_tool_name(tool))
            })?;
        let existing = if let Some(wsl_directory) = WslDirectory::parse(&location.directory) {
            wsl_directory
                .join(hook_config_filename(tool))
                .read_optional()?
        } else {
            read_optional_config(Path::new(&location.config_path))?
        };

        if existing
            .as_deref()
            .is_some_and(|content| hook_config_has_managed_marker(content, tool))
        {
            return Ok(());
        }

        self.write_hook_config(tool).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;
    use crate::domain::monitor::AiTool;

    fn test_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ai-monitor-auto-hooks-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn wait_for_file(path: &std::path::Path) {
        for _ in 0..200 {
            if path.exists() {
                return;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("等待自动 Hooks 写入超时：{}", path.display());
    }

    #[test]
    fn saving_ai_selection_writes_only_enabled_hooks_and_is_idempotent() {
        let root = test_root("selected");
        let app_data = root.join("app-data");
        let config_home = root.join("home");
        let service = MonitorService::load(&app_data, &config_home).unwrap();

        let settings = service.save_enabled_ai_tools(&[AiTool::Codex]).unwrap();
        assert_eq!(settings.enabled_ai_tools, vec![AiTool::Codex]);

        let codex_config = config_home.join(".codex/hooks.json");
        let claude_config = config_home.join(".claude/settings.json");
        wait_for_file(&codex_config);
        let first_content = fs::read_to_string(&codex_config).unwrap();
        assert!(first_content.contains("AIMonitor:tool=codex"));
        assert!(!claude_config.exists());

        let unchanged = service.write_hook_config(AiTool::Codex).unwrap();
        assert!(!unchanged.config_changed);

        let legacy_content =
            first_content.replace("--aimonitor-hook-relay", "--legacy-aimonitor-hook-relay");
        fs::write(&codex_config, &legacy_content).unwrap();
        service.auto_write_enabled_hook_configs();
        assert_eq!(fs::read_to_string(codex_config).unwrap(), legacy_content);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn one_invalid_hook_config_does_not_block_other_enabled_tools_or_settings() {
        let root = test_root("continue-after-error");
        let app_data = root.join("app-data");
        let config_home = root.join("home");
        let codex_config = config_home.join(".codex/hooks.json");
        fs::create_dir_all(codex_config.parent().unwrap()).unwrap();
        fs::write(&codex_config, "not valid json").unwrap();
        let service = MonitorService::load(&app_data, &config_home).unwrap();

        let settings = service
            .save_enabled_ai_tools(&[AiTool::Codex, AiTool::ClaudeCode])
            .unwrap();
        let claude_config_path = config_home.join(".claude/settings.json");
        wait_for_file(&claude_config_path);

        assert_eq!(
            settings.enabled_ai_tools,
            vec![AiTool::Codex, AiTool::ClaudeCode]
        );
        assert_eq!(fs::read_to_string(codex_config).unwrap(), "not valid json");
        let claude_config = fs::read_to_string(claude_config_path).unwrap();
        assert!(claude_config.contains("AIMonitor:tool=claude-code"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn automatic_hook_write_uses_the_saved_custom_directory() {
        let root = test_root("custom-directory");
        let app_data = root.join("app-data");
        let config_home = root.join("home");
        let custom_directory = root.join("custom-codex");
        let service = MonitorService::load(&app_data, &config_home).unwrap();
        service
            .save_hook_config_directory(AiTool::Codex, &custom_directory.to_string_lossy())
            .unwrap();

        service.save_enabled_ai_tools(&[AiTool::Codex]).unwrap();

        let custom_config = custom_directory.join("hooks.json");
        wait_for_file(&custom_config);
        let content = fs::read_to_string(custom_config).unwrap();
        assert!(content.contains("AIMonitor:tool=codex"));
        assert!(!config_home.join(".codex/hooks.json").exists());

        fs::remove_dir_all(root).unwrap();
    }
}
