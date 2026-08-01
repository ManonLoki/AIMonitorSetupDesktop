use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Barrier},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use super::*;
use crate::domain::monitor::{DiscoveredMonitorDevice, DiscoverySource};

fn loaded_service(test_name: &str) -> (MonitorService, PathBuf) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ai-monitor-{test_name}-{}-{unique}",
        std::process::id()
    ));
    let config_home = root.join("home");
    fs::create_dir_all(&config_home).unwrap();
    let service = MonitorService::load(&root.join("app-data"), &config_home).unwrap();
    (service, root)
}

fn device(id: &str, name: &str, address_suffix: u8) -> DiscoveredMonitorDevice {
    DiscoveredMonitorDevice {
        id: id.to_owned(),
        name: name.to_owned(),
        api_version: "1".to_owned(),
        base_url: format!("http://192.168.50.{address_suffix}:8080"),
        path: "/api/device".to_owned(),
        discovery_source: DiscoverySource::Mdns,
    }
}

fn assert_snapshot_invariant(snapshot: &MonitorDeviceSnapshot) {
    assert_eq!(
        snapshot.has_available_device,
        snapshot.current_device.is_some()
    );
    if let Some(current) = &snapshot.current_device {
        assert_eq!(current.id, snapshot.selected_device_id);
        assert!(snapshot.devices.iter().any(|device| device == current));
    }
    assert!(
        snapshot
            .other_devices
            .iter()
            .all(|device| device.id != snapshot.selected_device_id)
    );
}

#[test]
fn later_started_refresh_wins_when_older_scan_finishes_last() {
    let (service, root) = loaded_service("refresh-generation");
    let older_generation = service.begin_online_device_refresh().unwrap();
    let newer_generation = service.begin_online_device_refresh().unwrap();
    let newer_device = device("screen-new", "New", 20);

    let (newer_snapshot, changed) = service
        .update_online_devices(newer_generation, vec![newer_device.clone()])
        .unwrap();
    assert!(changed);
    assert_eq!(newer_snapshot.revision, 1);

    let (stale_snapshot, changed) = service
        .update_online_devices(older_generation, vec![device("screen-old", "Old", 10)])
        .unwrap();
    assert!(!changed);
    assert_eq!(stale_snapshot, newer_snapshot);
    assert_eq!(service.settings().unwrap().device_id, newer_device.id);
    assert_snapshot_invariant(&stale_snapshot);

    drop(service);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn manual_selection_rejects_offline_devices_and_uses_the_online_route() {
    let (service, root) = loaded_service("authoritative-selection");
    let desk = device("screen-1", "Desk", 10);
    let generation = service.begin_online_device_refresh().unwrap();
    let (initial, changed) = service
        .update_online_devices(generation, vec![desk.clone()])
        .unwrap();
    assert!(changed);
    assert_eq!(initial.revision, 1);

    let forged_route = device("screen-1", "Forged", 99);
    let selected = service.select_device_snapshot(&forged_route).unwrap();
    assert_eq!(selected.revision, 1);
    assert_eq!(selected.current_device, Some(desk.clone()));
    assert_eq!(
        selected
            .saved_device
            .as_ref()
            .map(|device| device.base_url.as_str()),
        Some(desk.base_url.as_str())
    );

    let error = service
        .select_device_snapshot(&device("offline", "Offline", 30))
        .unwrap_err();
    assert_eq!(error, "目标 AIMonitor 设备已离线，请刷新设备列表");
    assert_eq!(service.device_snapshot_state.lock().unwrap().revision, 1);

    drop(service);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn manual_selection_advances_snapshot_revision_and_notifies_hook_replay_watchers() {
    let (service, root) = loaded_service("selection-replay-watch");
    let mut revisions = service.hook_device_snapshot_revision.subscribe();
    assert_eq!(*revisions.borrow_and_update(), 0);

    service
        .select_device(&device("screen-1", "Desk", 10))
        .unwrap();
    assert!(revisions.has_changed().unwrap());
    assert_eq!(*revisions.borrow_and_update(), 1);

    *service.online_devices.write().unwrap() = vec![
        device("screen-1", "Desk", 10),
        device("screen-2", "Studio", 20),
    ];
    let snapshot = service
        .select_device_snapshot(&device("screen-2", "Forged", 99))
        .unwrap();
    assert_eq!(snapshot.revision, 2);
    assert!(revisions.has_changed().unwrap());
    assert_eq!(*revisions.borrow_and_update(), 2);

    drop(service);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn concurrent_refresh_commit_and_manual_selection_are_linearized() {
    let (service, root) = loaded_service("snapshot-linearization");
    let desk = device("screen-1", "Desk", 10);
    let studio = device("screen-2", "Studio", 20);
    let initial_generation = service.begin_online_device_refresh().unwrap();
    let (initial, changed) = service
        .update_online_devices(initial_generation, vec![desk.clone(), studio.clone()])
        .unwrap();
    assert!(changed);
    assert_eq!(initial.revision, 1);

    let refresh_generation = service.begin_online_device_refresh().unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let select_service = service.clone();
    let select_barrier = Arc::clone(&barrier);
    let selected_device = studio.clone();
    let select = thread::spawn(move || {
        select_barrier.wait();
        select_service
            .select_device_snapshot(&selected_device)
            .unwrap()
    });

    let refresh_service = service.clone();
    let refresh_barrier = Arc::clone(&barrier);
    let refresh = thread::spawn(move || {
        refresh_barrier.wait();
        refresh_service
            .update_online_devices(
                refresh_generation,
                vec![
                    device("screen-1", "Desk", 11),
                    device("screen-2", "Studio", 21),
                ],
            )
            .unwrap()
            .0
    });

    barrier.wait();
    let selected_snapshot = select.join().unwrap();
    let refresh_snapshot = refresh.join().unwrap();
    assert_snapshot_invariant(&selected_snapshot);
    assert_snapshot_invariant(&refresh_snapshot);

    let mut ordered = [selected_snapshot, refresh_snapshot];
    ordered.sort_by_key(|snapshot| snapshot.revision);
    assert_eq!(ordered[0].revision, 2);
    assert_eq!(ordered[1].revision, 3);
    assert_eq!(ordered[1].selected_device_id, "screen-2");
    if ordered[0].selected_device_id == "screen-2" {
        assert_eq!(ordered[1].selected_device_id, "screen-2");
    } else {
        assert_eq!(ordered[0].selected_device_id, "screen-1");
    }

    // 用已过期世代只读取当前已提交快照，不会更改状态或 revision。
    let (final_snapshot, changed) = service
        .update_online_devices(initial_generation, Vec::new())
        .unwrap();
    assert!(!changed);
    assert_eq!(final_snapshot.revision, 3);
    assert_snapshot_invariant(&final_snapshot);

    drop(service);
    fs::remove_dir_all(root).unwrap();
}
