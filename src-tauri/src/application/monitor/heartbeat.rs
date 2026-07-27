//! 控制端心跳发送：只向当前在线且至少配置了一个 Profile 的接收端续租。

use std::{collections::HashSet, thread};

use super::{HEARTBEAT_INTERVAL, MonitorService, build_hook_forward_client};
use crate::domain::monitor::{DiscoveredMonitorDevice, SavedMonitorData};

impl MonitorService {
    /// 启动独立后台线程。单台失败不会阻止其他设备，也不会污染 Hook 投递统计。
    pub fn start_heartbeat_sender(&self) {
        let service = self.clone();
        thread::Builder::new()
            .name("aimonitor-heartbeat".to_owned())
            .spawn(move || {
                let client = match build_hook_forward_client() {
                    Ok(client) => client,
                    Err(error) => {
                        eprintln!("无法创建心跳客户端：{error}");
                        return;
                    }
                };
                loop {
                    service.send_heartbeats_once(&client);
                    thread::sleep(HEARTBEAT_INTERVAL);
                }
            })
            .expect("无法启动心跳线程");
    }

    fn send_heartbeats_once(&self, client: &reqwest::blocking::Client) {
        // 计算完目标后立即让读锁在此块结束时释放，发 HTTP 请求前不占着任何锁。
        let (client_id, targets) = {
            let Ok(data) = self.data.read() else {
                return;
            };
            let Ok(online_devices) = self.online_devices.read() else {
                return;
            };
            heartbeat_targets(&data, &online_devices)
        };

        thread::scope(|scope| {
            for base_url in targets {
                let client_id = client_id.as_str();
                scope.spawn(move || {
                    let url = format!("{base_url}/api/clients/{client_id}/heartbeat");
                    let _ = client.post(url).send();
                });
            }
        });
    }
}

fn heartbeat_targets(
    data: &SavedMonitorData,
    online_devices: &[DiscoveredMonitorDevice],
) -> (String, Vec<String>) {
    let configured_ids = data
        .profiles
        .iter()
        .map(|profile| profile.device_id.as_str())
        .collect::<HashSet<_>>();
    let targets = online_devices
        .iter()
        .filter(|device| configured_ids.contains(device.id.as_str()))
        .map(|device| device.base_url.clone())
        .collect();
    (data.client_id.clone(), targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::monitor::test_support::test_profile,
        domain::monitor::{DiscoverySource, MonitorSettings},
    };

    #[test]
    fn targets_only_online_devices_with_profiles() {
        let data = SavedMonitorData {
            client_id: "controller-1".into(),
            profiles: vec![test_profile()],
            settings: MonitorSettings::default(),
            ..SavedMonitorData::default()
        };
        let online = vec![
            device("screen-1", "http://screen-1"),
            device("screen-2", "http://screen-2"),
        ];

        let (client_id, targets) = heartbeat_targets(&data, &online);

        assert_eq!(client_id, "controller-1");
        assert_eq!(targets, vec!["http://screen-1"]);
        assert_eq!(HEARTBEAT_INTERVAL, std::time::Duration::from_secs(30));
    }

    fn device(id: &str, base_url: &str) -> DiscoveredMonitorDevice {
        DiscoveredMonitorDevice {
            id: id.into(),
            name: id.into(),
            api_version: "3".into(),
            base_url: base_url.into(),
            path: "/api/device".into(),
            discovery_source: DiscoverySource::Mdns,
        }
    }
}
