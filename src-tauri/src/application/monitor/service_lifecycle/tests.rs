use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Client;

use super::*;
use crate::{
    application::monitor::test_support::test_profile,
    domain::monitor::{
        AiProfileDraft, AiTool, DEFAULT_PROFILE_SLOT, DiscoveredMonitorDevice, DiscoverySource,
        HookConfigDirectories, HookWriteOutcome, MonitorSettings,
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
    let saved: SavedMonitorData =
        serde_json::from_str(&fs::read_to_string(app_data.join(STORE_FILENAME)).unwrap()).unwrap();
    assert!(!saved.client_id.is_empty());
    let reloaded = MonitorService::load(&app_data, &config_home).unwrap();
    assert_eq!(reloaded.data.read().unwrap().client_id, saved.client_id);
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
            qwen_code: config_home.join(".qwen").to_string_lossy().into_owned(),
            kimi_code: config_home
                .join(".kimi-code")
                .to_string_lossy()
                .into_owned(),
            qoder: config_home.join(".qoder").to_string_lossy().into_owned(),
            gemini_cli: config_home.join(".gemini").to_string_lossy().into_owned(),
            github_copilot: config_home.join(".copilot").to_string_lossy().into_owned(),
        },
        data: Arc::new(RwLock::new(SavedMonitorData {
            client_id: "test-client".to_owned(),
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
        device_snapshot_state: Arc::new(Mutex::new(DeviceSnapshotState::default())),
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
    let write_result = service.write_hook_config(AiTool::Codex).unwrap();
    assert_eq!(write_result.outcome, HookWriteOutcome::CodexReviewRequired);
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
    assert_eq!(result.outcome, HookWriteOutcome::OpenClawEnableRequired);
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
    assert_eq!(result.outcome, HookWriteOutcome::HermesEnableRequired);
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
    assert_eq!(unchanged.outcome, HookWriteOutcome::Unchanged);
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

    assert!(
        service
            .save_hook_config_directory(AiTool::Cursor, "relative/path")
            .is_err()
    );
    assert!(
        service
            .save_hook_config_directory(AiTool::ClaudeCode, &file_path.to_string_lossy())
            .is_err()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn profile_drafts_use_defaults_and_reject_a_stale_device_token() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ai-monitor-profile-drafts-{}-{unique}",
        std::process::id()
    ));
    let service = MonitorService::load(&root.join("app-data"), &root.join("home")).unwrap();

    let defaults = service.profile_drafts().unwrap();
    let default_tools = defaults
        .drafts
        .iter()
        .map(|draft| draft.tool)
        .collect::<Vec<_>>();
    assert_eq!(default_tools, AiTool::ALL);
    assert!(
        defaults
            .drafts
            .iter()
            .all(|draft| draft.slot == DEFAULT_PROFILE_SLOT)
    );

    let current_device = DiscoveredMonitorDevice {
        id: "current-screen".to_owned(),
        name: "Desk".to_owned(),
        api_version: "1".to_owned(),
        base_url: "http://127.0.0.1:8080".to_owned(),
        path: "/api/device".to_owned(),
        discovery_source: DiscoverySource::Mdns,
    };
    service.select_device(&current_device).unwrap();
    let mut codex = AiProfileDraft::from(test_profile());
    codex.slot = 9;
    codex.hooks[0].content = "  resting  ".to_owned();
    codex.hooks[0].image = "  idle.png  ".to_owned();
    let expected_device_id = service.profile_drafts().unwrap().expected_device_id;
    let saved = service
        .save_profile_draft(&expected_device_id, codex)
        .unwrap();
    assert_eq!(saved.slot, 9);
    assert_eq!(saved.hooks[0].content, "resting");
    assert_eq!(saved.hooks[0].image, "idle.png");

    let stored = service.data.read().unwrap();
    assert_eq!(stored.profiles[0].device_id, "current-screen");
    drop(stored);
    let drafts = service.profile_drafts().unwrap();
    assert_eq!(drafts.drafts[0], saved);
    assert_eq!(drafts.drafts[1].tool, AiTool::ClaudeCode);
    assert_eq!(drafts.drafts[1].slot, DEFAULT_PROFILE_SLOT);

    let invalid = AiProfileDraft::default_for(AiTool::Cursor);
    assert!(
        service
            .save_profile_draft(&expected_device_id, invalid)
            .is_err()
    );
    assert_eq!(service.profiles().unwrap().len(), 1);
    let stale = service.profile_drafts().unwrap();
    let mut next_device = current_device;
    next_device.id = "next-screen".to_owned();
    next_device.base_url = "http://127.0.0.1:8081".to_owned();
    service.select_device(&next_device).unwrap();
    assert!(
        service
            .save_profile_draft(
                &stale.expected_device_id,
                AiProfileDraft::from(test_profile())
            )
            .is_err()
    );
    assert!(service.profiles().unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();
}
