use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Client;

use super::*;
use crate::{
    application::monitor::test_support::test_profile,
    domain::monitor::{
        AiTool, DiscoveredMonitorDevice, DiscoverySource, HookConfigDirectories, MonitorSettings,
    },
};

#[test]
fn empty_username_defaults_to_the_local_system_username() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ai-monitor-default-username-{}-{unique}",
        std::process::id()
    ));
    let app_data = root.join("app-data");
    let config_home = root.join("fallback-user");
    let expected = detect_system_username(&config_home).unwrap();

    let service = MonitorService::load(&app_data, &config_home).unwrap();

    assert_eq!(service.settings().unwrap().username, expected);
    fs::remove_dir_all(root).unwrap();
}

// 验证保存 Profile 时若持久化到磁盘失败（此处故意把 data_path 指向一个目录而非文件，
// 制造写入失败），不会产生"内存已更新但磁盘未写入"的不一致状态：
// save_profile 应返回错误，且内存中的 profiles 仍应为空，Hook 配置文件也不应被生成。
#[test]
fn profile_save_does_not_write_hooks_when_data_persistence_fails() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ai-monitor-hook-transaction-{}-{unique}",
        std::process::id()
    ));
    let config_home = root.join("home");
    // 故意让 data_path 指向一个目录（而非文件），这样写入配置时必然失败。
    let invalid_data_path = root.join("data-path-is-a-directory");
    fs::create_dir_all(&invalid_data_path).unwrap();

    // 手工构造 MonitorService（而非走 load()），以便注入这个必然失败的 data_path。
    let service = MonitorService {
        client: Client::new(),
        data_path: invalid_data_path,
        default_hook_config_directories: HookConfigDirectories {
            codex: config_home.join(".codex").to_string_lossy().into_owned(),
            claude_code: config_home.join(".claude").to_string_lossy().into_owned(),
            cursor: config_home.join(".cursor").to_string_lossy().into_owned(),
            open_code: config_home
                .join(".config/opencode")
                .to_string_lossy()
                .into_owned(),
            work_buddy: config_home
                .join(".workbuddy")
                .to_string_lossy()
                .into_owned(),
            hermes: config_home.join(".hermes").to_string_lossy().into_owned(),
            open_claw: config_home.join(".openclaw").to_string_lossy().into_owned(),
            code_buddy: config_home
                .join(".codebuddy")
                .to_string_lossy()
                .into_owned(),
        },
        data: Arc::new(RwLock::new(SavedMonitorData {
            settings: MonitorSettings {
                base_url: "http://127.0.0.1:8080".to_owned(),
                username: "tester".to_owned(),
                device_id: "device-1".to_owned(),
                device_name: "monitor".to_owned(),
                ..MonitorSettings::default()
            },
            devices: Vec::new(),
            profiles: Vec::new(),
            hook_config_directories: HookConfigDirectories::default(),
        })),
        online_devices: Arc::new(RwLock::new(Vec::new())),
        discovery_missed_scans: Arc::new(Mutex::new(HashMap::new())),
        hook_config_write_lock: Arc::new(Mutex::new(())),
        relay_status: Arc::new(RwLock::new(HookRelayStatus::default())),
    };

    let result = service.save_profile(test_profile());

    // 保存应失败，且没有留下任何副作用：既没写 hooks.json，内存里的 profiles 也仍为空。
    assert!(result.is_err());
    assert!(!config_home.join(".codex/hooks.json").exists());
    assert!(service.profiles().unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();
}

// 验证自定义 Hook 配置目录能被正确持久化，并且 write_hook_config 会写入到该自定义目录
// 而非默认目录；重新加载服务后自定义目录设置依然生效；清空自定义目录后能恢复默认目录。
#[test]
fn custom_hook_directory_is_persisted_and_used_for_hook_writes() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ai-monitor-hook-directory-{}-{unique}",
        std::process::id()
    ));
    let app_data = root.join("app-data");
    let config_home = root.join("home");
    fs::create_dir_all(&app_data).unwrap();
    let service = MonitorService::load(&app_data, &config_home).unwrap();
    service
        .select_device(&DiscoveredMonitorDevice {
            id: "screen-1".to_owned(),
            name: "Desk".to_owned(),
            api_version: "1".to_owned(),
            base_url: "http://127.0.0.1:8080".to_owned(),
            path: "/api/device".to_owned(),
            discovery_source: DiscoverySource::Mdns,
        })
        .unwrap();
    let custom_directory = root.join("custom-codex");
    // 记录下切换自定义目录之前系统探测到的默认目录，后面用于验证"清空自定义目录后能恢复默认值"。
    let detected_directory = service
        .hook_config_locations()
        .unwrap()
        .into_iter()
        .find(|item| item.tool == AiTool::Codex)
        .unwrap()
        .directory;

    // 保存自定义目录。
    let location = service
        .save_hook_config_directory(AiTool::Codex, &custom_directory.to_string_lossy())
        .unwrap();

    // 保存后应标记为自定义，且配置文件路径应指向自定义目录下的 hooks.json。
    assert!(location.is_custom);
    assert_eq!(
        PathBuf::from(&location.config_path),
        custom_directory.join("hooks.json")
    );
    // Hooks 不依赖设备展示 Profile；首次配置时可以先完成写入。
    assert!(service.profiles().unwrap().is_empty());
    assert!(!custom_directory.join("hooks.json").exists());
    service.write_hook_config(AiTool::Codex).unwrap();
    // 写入后应出现在自定义目录，而不是默认目录。
    assert!(custom_directory.join("hooks.json").exists());
    assert!(!config_home.join(".codex/hooks.json").exists());
    // 重新加载服务（模拟重启），自定义目录设置应仍然生效。
    let reloaded = MonitorService::load(&app_data, &config_home).unwrap();
    let reloaded_location = reloaded
        .hook_config_locations()
        .unwrap()
        .into_iter()
        .find(|item| item.tool == AiTool::Codex)
        .unwrap();
    assert_eq!(reloaded_location.directory, location.directory);
    assert!(reloaded_location.is_custom);

    // 传入空字符串应恢复为默认探测目录，且不再标记为自定义。
    let default_location = reloaded
        .save_hook_config_directory(AiTool::Codex, "")
        .unwrap();
    assert!(!default_location.is_custom);
    assert_eq!(default_location.directory, detected_directory);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn startup_migration_replaces_legacy_windows_relay_without_touching_user_only_files() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ai-monitor-hook-migration-{}-{unique}",
        std::process::id()
    ));
    let app_data = root.join("app-data");
    let config_home = root.join("home");
    let codex_dir = config_home.join(".codex");
    let claude_dir = config_home.join(".claude");
    fs::create_dir_all(&app_data).unwrap();
    fs::create_dir_all(&codex_dir).unwrap();
    fs::create_dir_all(&claude_dir).unwrap();
    let legacy = r#"{
      "hooks": {
        "PostToolUse": [{
          "hooks": [{
            "type": "command",
            "command": "other-app notify",
            "commandWindows": ": 'AIMonitor|tool=codex'; powershell.exe Invoke-RestMethod -Body $body"
          }]
        }]
      }
    }"#;
    let user_only = r#"{"hooks":{"Stop":[{"hooks":[{"command":"my notifier"}]}]}}"#;
    fs::write(codex_dir.join("hooks.json"), legacy).unwrap();
    fs::write(claude_dir.join("settings.json"), user_only).unwrap();
    let service = MonitorService::load(&app_data, &config_home).unwrap();

    assert_eq!(service.migrate_existing_managed_hook_configs().unwrap(), 1);
    let migrated = fs::read_to_string(codex_dir.join("hooks.json")).unwrap();
    assert!(migrated.contains("--aimonitor-hook-relay"));
    assert!(!migrated.contains("Invoke-RestMethod"));
    assert_eq!(
        fs::read_to_string(claude_dir.join("settings.json")).unwrap(),
        user_only
    );
    assert_eq!(service.migrate_existing_managed_hook_configs().unwrap(), 0);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn open_claw_plugin_files_are_validated_as_one_managed_set() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ai-monitor-openclaw-plugin-{}-{unique}",
        std::process::id()
    ));
    let app_data = root.join("app-data");
    let config_home = root.join("home");
    fs::create_dir_all(&app_data).unwrap();
    let service = MonitorService::load(&app_data, &config_home).unwrap();
    service
        .select_device(&DiscoveredMonitorDevice {
            id: "screen-1".to_owned(),
            name: "Desk".to_owned(),
            api_version: "1".to_owned(),
            base_url: "http://127.0.0.1:8080".to_owned(),
            path: "/api/device".to_owned(),
            discovery_source: DiscoverySource::Mdns,
        })
        .unwrap();
    let mut profile = test_profile();
    profile.tool = AiTool::OpenClaw;
    service.save_profile(profile).unwrap();
    let plugin_root = root.join("openclaw");
    service
        .save_hook_config_directory(AiTool::OpenClaw, &plugin_root.to_string_lossy())
        .unwrap();

    // 任一辅助文件已被用户占用时，主入口和其他文件都不应提前写入。
    let package_path = plugin_root.join("extensions/aimonitor/package.json");
    fs::create_dir_all(package_path.parent().unwrap()).unwrap();
    fs::write(&package_path, r#"{"name":"unrelated"}"#).unwrap();
    assert!(service.write_hook_config(AiTool::OpenClaw).is_err());
    assert!(!plugin_root.join("extensions/aimonitor/index.mjs").exists());
    assert!(
        !plugin_root
            .join("extensions/aimonitor/openclaw.plugin.json")
            .exists()
    );

    fs::remove_file(&package_path).unwrap();
    let result = service.write_hook_config(AiTool::OpenClaw).unwrap();
    assert!(result.config_changed);
    assert!(result.requires_review);
    assert!(result.restart_required);
    assert!(plugin_root.join("extensions/aimonitor/index.mjs").exists());
    assert!(
        plugin_root
            .join("extensions/aimonitor/openclaw.plugin.json")
            .exists()
    );
    assert!(package_path.exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hermes_plugin_is_written_as_one_managed_set() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ai-monitor-hermes-plugin-{}-{unique}",
        std::process::id()
    ));
    let app_data = root.join("app-data");
    let config_home = root.join("home");
    fs::create_dir_all(&app_data).unwrap();
    let service = MonitorService::load(&app_data, &config_home).unwrap();
    let hermes_home = root.join("hermes-home");
    service
        .save_hook_config_directory(AiTool::Hermes, &hermes_home.to_string_lossy())
        .unwrap();

    let result = service.write_hook_config(AiTool::Hermes).unwrap();
    assert!(result.config_changed);
    assert!(result.requires_review);
    assert!(result.restart_required);
    let plugin_root = hermes_home.join("plugins/aimonitor");
    let entrypoint = fs::read_to_string(plugin_root.join("__init__.py")).unwrap();
    let manifest = fs::read_to_string(plugin_root.join("plugin.yaml")).unwrap();
    assert!(entrypoint.contains("AIMonitor:tool=hermes"));
    assert!(entrypoint.contains("/api/hooks/hermes"));
    assert!(manifest.contains("name: aimonitor"));

    let unchanged = service.write_hook_config(AiTool::Hermes).unwrap();
    assert!(!unchanged.config_changed);
    assert!(!unchanged.requires_review);
    assert!(!unchanged.restart_required);
    fs::remove_dir_all(root).unwrap();
}

// 验证保存 Hook 配置目录时会拒绝相对路径，也会拒绝指向一个已存在普通文件（而非目录）的路径。
#[test]
fn hook_directory_rejects_relative_and_file_paths() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ai-monitor-hook-directory-validation-{}-{unique}",
        std::process::id()
    ));
    let app_data = root.join("app-data");
    let config_home = root.join("home");
    fs::create_dir_all(&app_data).unwrap();
    let service = MonitorService::load(&app_data, &config_home).unwrap();
    let file_path = root.join("not-a-directory");
    fs::write(&file_path, "content").unwrap();

    // 相对路径应被拒绝。
    assert!(
        service
            .save_hook_config_directory(AiTool::Cursor, "relative/path")
            .is_err()
    );
    // 指向一个已存在的普通文件（非目录）也应被拒绝。
    assert!(
        service
            .save_hook_config_directory(AiTool::ClaudeCode, &file_path.to_string_lossy())
            .is_err()
    );
    fs::remove_dir_all(root).unwrap();
}
