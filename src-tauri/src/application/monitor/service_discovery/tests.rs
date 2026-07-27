use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use super::*;
use crate::{
    application::monitor::test_support::test_profile,
    domain::monitor::{AiTool, DiscoveredMonitorDevice, DiscoverySource},
};

// 验证切换当前设备后，profiles() 只返回新设备的 Profile（而不是混合了旧设备的）；
// 同时验证切回旧设备后能重新看到旧设备的 Profile，且历史设备记录、用户名等不会丢失。
#[test]
fn switching_current_device_loads_that_devices_profiles() {
    // 用当前时间戳+进程号构造一个唯一的临时目录，避免测试间相互干扰。
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ai-monitor-route-refresh-{}-{unique}",
        std::process::id()
    ));
    let app_data = root.join("app-data");
    let config_home = root.join("home");
    fs::create_dir_all(&app_data).unwrap();
    let service = MonitorService::load(&app_data, &config_home).unwrap();
    service.save_username("Manon").unwrap();
    // 选中设备一（screen-1）并保存一个 Profile。
    service
        .select_device(&DiscoveredMonitorDevice {
            id: "screen-1".to_owned(),
            name: "Desk".to_owned(),
            api_version: "1".to_owned(),
            base_url: "http://192.168.50.10:8080".to_owned(),
            path: "/api/device".to_owned(),
            discovery_source: DiscoverySource::Mdns,
        })
        .unwrap();
    service.save_profile(test_profile()).unwrap();

    // 切换到设备二（screen-2）。
    service
        .select_device(&DiscoveredMonitorDevice {
            id: "screen-2".to_owned(),
            name: "Studio".to_owned(),
            api_version: "1".to_owned(),
            base_url: "http://192.168.50.99:8080".to_owned(),
            path: "/api/device".to_owned(),
            discovery_source: DiscoverySource::Mdns,
        })
        .unwrap();

    // 设备二还没有保存过任何 Profile，应为空列表。
    assert!(service.profiles().unwrap().is_empty());
    let mut studio_profile = test_profile();
    studio_profile.slot = 9;
    service.save_profile(studio_profile).unwrap();
    let saved_studio_profile = service.profiles().unwrap().remove(0);
    assert_eq!(saved_studio_profile.device_id, "screen-2");
    assert_eq!(saved_studio_profile.slot, 9);
    // 切回设备一。
    service
        .select_device(&DiscoveredMonitorDevice {
            id: "screen-1".to_owned(),
            name: "Desk".to_owned(),
            api_version: "1".to_owned(),
            base_url: "http://192.168.50.10:8080".to_owned(),
            path: "/api/device".to_owned(),
            discovery_source: DiscoverySource::Mdns,
        })
        .unwrap();
    // 应该重新看到设备一之前保存的 Profile（槽位 1）。
    let profile = service.profiles().unwrap().remove(0);
    assert_eq!(profile.tool, AiTool::Codex);
    assert_eq!(profile.device_id, "screen-1");
    assert_eq!(profile.slot, 1);
    // 用户名在切换设备过程中应保持不变。
    assert_eq!(service.settings().unwrap().username, "Manon");
    // 历史设备列表应同时保留两台设备的记录。
    let saved = service.data.read().unwrap();
    assert_eq!(saved.devices.len(), 2);
    assert!(
        saved
            .devices
            .iter()
            .any(|device| device.device_id == "screen-2")
    );
    fs::remove_dir_all(root).unwrap();
}

// 验证当前选中设备不在在线列表中时会自动切换到第一台在线设备，
// 并且这次自动切换会被持久化（重新加载服务后仍是新设备）。
#[test]
fn unavailable_current_device_switches_to_first_online_device_and_persists() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ai-monitor-auto-select-{}-{unique}",
        std::process::id()
    ));
    let app_data = root.join("app-data");
    let config_home = root.join("home");
    fs::create_dir_all(&config_home).unwrap();
    let service = MonitorService::load(&app_data, &config_home).unwrap();
    let current = DiscoveredMonitorDevice {
        id: "screen-1".to_owned(),
        name: "Desk".to_owned(),
        api_version: "1".to_owned(),
        base_url: "http://192.168.50.10:8080".to_owned(),
        path: "/api/device".to_owned(),
        discovery_source: DiscoverySource::Mdns,
    };
    let next = DiscoveredMonitorDevice {
        id: "screen-2".to_owned(),
        name: "Studio".to_owned(),
        api_version: "1".to_owned(),
        base_url: "http://192.168.50.99:8080".to_owned(),
        path: "/api/device".to_owned(),
        discovery_source: DiscoverySource::Mdns,
    };
    service.select_device(&current).unwrap();

    // 在线列表为空：不应触发切换，仍然选中原设备。
    assert!(
        !service
            .select_first_available_device_if_needed(&[])
            .unwrap()
    );
    assert_eq!(service.settings().unwrap().device_id, "screen-1");
    // 在线列表只包含另一台设备（不含当前选中设备）：应触发切换。
    assert!(
        service
            .select_first_available_device_if_needed(std::slice::from_ref(&next))
            .unwrap()
    );
    assert_eq!(service.settings().unwrap().device_id, "screen-2");

    // 重新加载服务（模拟应用重启），验证切换结果已被持久化。
    drop(service);
    let reloaded = MonitorService::load(&app_data, &config_home).unwrap();
    assert_eq!(reloaded.settings().unwrap().device_id, "screen-2");
    drop(reloaded);
    fs::remove_dir_all(root).unwrap();
}

// 验证发现间隔的默认值、非法值拒绝以及保存后立即生效（无需重启服务）。
#[test]
fn discovery_interval_is_saved_and_read_back_without_restarting_the_service() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ai-monitor-discovery-interval-{}-{unique}",
        std::process::id()
    ));
    let app_data = root.join("app-data");
    let config_home = root.join("home");
    fs::create_dir_all(&app_data).unwrap();
    let service = MonitorService::load(&app_data, &config_home).unwrap();

    // 默认间隔应为 1 分钟。
    assert_eq!(service.discovery_interval(), Duration::from_mins(1));
    // 0 分钟是非法值，应被拒绝。
    assert!(service.save_discovery_interval(0).is_err());

    // 保存 15 分钟后，无需重启服务即可立即读到新值。
    let updated = service.save_discovery_interval(15).unwrap();
    assert_eq!(updated.discovery_interval_minutes, 15);
    assert_eq!(service.discovery_interval(), Duration::from_mins(15));
}
