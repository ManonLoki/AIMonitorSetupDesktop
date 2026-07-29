// 探测各 AI 工具的 Hook 配置目录，以及从系统环境推断默认用户名；
// 供 service_lifecycle 的 `MonitorService::load` 在首次加载时使用。
use std::path::{Path, PathBuf};

use crate::domain::monitor::{HookConfigDirectories, validate_username};

// 根据用户主目录与工具公开的环境变量，探测各 AI 工具的 Hook 配置目录。
pub(super) fn detect_hook_config_directories(config_home: &Path) -> HookConfigDirectories {
    let open_code_fallback = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| config_home.join(".config"))
        .join("opencode");
    HookConfigDirectories {
        // Codex 配置目录：优先读取环境变量 CODEX_HOME，否则回退到 ~/.codex。
        codex: detected_config_directory("CODEX_HOME", &config_home.join(".codex")),
        // Claude Code 配置目录：优先读取环境变量 CLAUDE_CONFIG_DIR，否则回退到 ~/.claude。
        claude_code: detected_config_directory("CLAUDE_CONFIG_DIR", &config_home.join(".claude")),
        // Cursor 配置目录固定为 ~/.cursor（没有对应的环境变量覆盖）。
        cursor: config_home.join(".cursor").to_string_lossy().into_owned(),
        // OpenCode 支持 OPENCODE_CONFIG_DIR；否则遵循 XDG_CONFIG_HOME/opencode。
        open_code: detected_config_directory("OPENCODE_CONFIG_DIR", &open_code_fallback),
        // WorkBuddy 自 v2.48 起与 CodeBuddy CLI 分离，使用 ~/.workbuddy。
        work_buddy: config_home
            .join(".workbuddy")
            .to_string_lossy()
            .into_owned(),
        // Hermes 支持 HERMES_HOME；默认位置在 POSIX 为 ~/.hermes，Windows 为 %LOCALAPPDATA%\hermes。
        hermes: detected_config_directory("HERMES_HOME", &default_hermes_home(config_home)),
        // OpenClaw 的可变状态目录可由 OPENCLAW_STATE_DIR 覆盖。
        open_claw: detected_config_directory("OPENCLAW_STATE_DIR", &config_home.join(".openclaw")),
        // CodeBuddy 官方通过 CODEBUDDY_CONFIG_DIR 覆盖 ~/.codebuddy。
        code_buddy: detected_config_directory(
            "CODEBUDDY_CONFIG_DIR",
            &config_home.join(".codebuddy"),
        ),
        // Qwen Code 与 Qoder 的用户级配置目录均为公开固定位置。
        qwen_code: config_home.join(".qwen").to_string_lossy().into_owned(),
        // Kimi Code 支持 KIMI_CODE_HOME；否则使用 ~/.kimi-code。
        kimi_code: detected_config_directory("KIMI_CODE_HOME", &config_home.join(".kimi-code")),
        qoder: config_home.join(".qoder").to_string_lossy().into_owned(),
        // Gemini CLI 的用户设置固定放在 ~/.gemini。
        gemini_cli: config_home.join(".gemini").to_string_lossy().into_owned(),
        // Copilot CLI 支持 COPILOT_HOME；AIMonitor 在其 hooks 子目录写独立文件。
        github_copilot: detected_config_directory("COPILOT_HOME", &config_home.join(".copilot")),
    }
}

// Windows 上 Hermes 默认安装在 %LOCALAPPDATA%\hermes。
#[cfg(target_os = "windows")]
fn default_hermes_home(config_home: &Path) -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| config_home.to_owned())
        .join("hermes")
}

// POSIX 系统上 Hermes 默认安装在 ~/.hermes。
#[cfg(not(target_os = "windows"))]
fn default_hermes_home(config_home: &Path) -> PathBuf {
    config_home.join(".hermes")
}

// 读取指定环境变量作为配置目录，若变量不存在或不是绝对路径则使用回退路径。
fn detected_config_directory(variable: &str, fallback: &Path) -> String {
    std::env::var_os(variable)
        .map(PathBuf::from)
        // 只信任绝对路径的环境变量值，避免相对路径造成歧义。
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| fallback.to_owned())
        .to_string_lossy()
        .into_owned()
}

/// 从系统环境与主目录名中取得当前本机用户名。环境变量在桌面启动环境缺失时，
/// 主目录末级名称仍可覆盖 macOS/Linux/Windows 的常见用户目录布局。
pub(super) fn detect_system_username(config_home: &Path) -> Option<String> {
    ["USER", "USERNAME", "LOGNAME"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .chain(
            config_home
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned),
        )
        .find_map(|candidate| validate_username(&candidate).ok())
}
