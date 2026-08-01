use serde::Serialize;

use crate::domain::monitor::{DiscoveredMonitorDevice, MonitorSettings};

/// 当前设备选择与在线列表的原子业务快照。发现 command 与后台事件复用同一
/// DTO，前端无需再把独立的 settings/devices Query 按时序拼接。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorDeviceSnapshot {
    /// 进程内单调的设备业务快照修订号；只有选择或在线列表发生
    /// 实际变化时才推进，供前端拒绝迟到事件。
    pub revision: u64,
    pub devices: Vec<DiscoveredMonitorDevice>,
    pub current_device: Option<DiscoveredMonitorDevice>,
    pub other_devices: Vec<DiscoveredMonitorDevice>,
    pub selected_device_id: String,
    pub saved_device: Option<SavedMonitorDevice>,
    pub has_configured_device: bool,
    pub has_available_device: bool,
}

/// 当前持久化选择的最小路由信息。即使设备暂时离线，连接面板也可以展示这份
/// Rust 事实；在线与否由 `current_device` 明确表达。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedMonitorDevice {
    pub id: String,
    pub name: String,
    pub base_url: String,
}

impl MonitorDeviceSnapshot {
    pub(super) fn from_parts(
        revision: u64,
        settings: MonitorSettings,
        devices: Vec<DiscoveredMonitorDevice>,
    ) -> Self {
        let has_configured_device = !settings.device_id.is_empty();
        let current_device = devices
            .iter()
            .find(|device| device.id == settings.device_id)
            .cloned();
        let other_devices = devices
            .iter()
            .filter(|device| device.id != settings.device_id)
            .cloned()
            .collect();
        let saved_device = has_configured_device.then(|| SavedMonitorDevice {
            id: settings.device_id.clone(),
            name: settings.device_name,
            base_url: settings.base_url,
        });
        let has_available_device = current_device.is_some();

        Self {
            revision,
            has_available_device,
            devices,
            current_device,
            other_devices,
            selected_device_id: settings.device_id,
            saved_device,
            has_configured_device,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::monitor::DiscoverySource;

    fn device(id: &str, name: &str) -> DiscoveredMonitorDevice {
        DiscoveredMonitorDevice {
            id: id.to_owned(),
            name: name.to_owned(),
            api_version: "1".to_owned(),
            base_url: format!("http://{id}:8080"),
            path: "/api/device".to_owned(),
            discovery_source: DiscoverySource::Mdns,
        }
    }

    #[test]
    fn snapshot_classifies_current_and_other_devices_from_one_settings_version() {
        let settings = MonitorSettings {
            device_id: "screen-2".to_owned(),
            device_name: "Studio".to_owned(),
            base_url: "http://old-address:8080".to_owned(),
            ..MonitorSettings::default()
        };
        let snapshot = MonitorDeviceSnapshot::from_parts(
            7,
            settings,
            vec![device("screen-1", "Desk"), device("screen-2", "Studio")],
        );

        assert_eq!(snapshot.revision, 7);
        assert_eq!(
            snapshot
                .current_device
                .as_ref()
                .map(|device| device.id.as_str()),
            Some("screen-2")
        );
        assert_eq!(snapshot.other_devices.len(), 1);
        assert_eq!(snapshot.other_devices[0].id, "screen-1");
        assert!(snapshot.has_configured_device);
        assert!(snapshot.has_available_device);
        assert_eq!(
            snapshot
                .saved_device
                .as_ref()
                .map(|device| device.base_url.as_str()),
            Some("http://old-address:8080")
        );
    }

    #[test]
    fn offline_saved_device_stays_explicit_without_becoming_current() {
        let settings = MonitorSettings {
            device_id: "offline".to_owned(),
            device_name: "Offline".to_owned(),
            base_url: "http://offline:8080".to_owned(),
            ..MonitorSettings::default()
        };
        let snapshot = MonitorDeviceSnapshot::from_parts(3, settings, vec![]);

        assert_eq!(snapshot.revision, 3);
        assert!(snapshot.current_device.is_none());
        assert!(snapshot.saved_device.is_some());
        assert!(snapshot.has_configured_device);
        assert!(!snapshot.has_available_device);
    }

    #[test]
    fn non_current_devices_do_not_make_the_selected_device_available() {
        let settings = MonitorSettings {
            device_id: "offline".to_owned(),
            device_name: "Offline".to_owned(),
            base_url: "http://offline:8080".to_owned(),
            ..MonitorSettings::default()
        };
        let snapshot =
            MonitorDeviceSnapshot::from_parts(11, settings, vec![device("screen-1", "Desk")]);

        assert!(snapshot.current_device.is_none());
        assert_eq!(snapshot.other_devices.len(), 1);
        assert!(!snapshot.has_available_device);
    }
}
