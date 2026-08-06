// 设备连接健康检查：主动检测（用户触发，超时更宽松）与批量发现时的
// 快速可达性探测（超时更短，供 mDNS/UDP 候选逐个探测使用）。
use std::time::Duration;

use serde::Serialize;
use tracing::warn;

use super::{DISCOVERY_PROBE_TIMEOUT, MonitorService};
use crate::domain::AppError;
use crate::domain::monitor::{DiscoveredMonitorDevice, normalize_base_url};

// 用户主动触发的连接检测比批量发现探测更宽松：偶发触发、无需批量并发扫描，
// 值得多等一会儿以减少误报“不可达”。
const CONNECTION_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

// 连接测试结果，返回给前端展示设备是否可达。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub reachable: bool,
    pub base_url: String,
    pub message: String,
}

impl MonitorService {
    /// 从当前在线快照中取出 UI 当前选中的设备。只按稳定的设备 ID
    /// 匹配，因此返回的名称和地址都来自设备发现的最新快照，而不是可能
    /// 已经过期的持久化路由。当前设备不在快照时直接拒绝设备业务请求。
    pub(super) fn current_online_device(&self) -> Result<DiscoveredMonitorDevice, AppError> {
        // 与发现提交/手动切换共用事务锁，避免在读取 device_id 后、
        // 查找在线路由前被切换打断，从而让旧 expectedDeviceId 错误通过。
        let _snapshot_transaction = self.lock_device_snapshot_transaction()?;
        self.current_online_device_locked()
    }

    /// 调用方必须已持有设备快照事务锁。
    fn current_online_device_locked(&self) -> Result<DiscoveredMonitorDevice, AppError> {
        let device_id = self.settings()?.device_id;
        if device_id.is_empty() {
            return Err(AppError::new("error.connection.deviceNotSelected"));
        }

        self.online_devices
            .read()
            .map_err(|_| AppError::new("error.internal.lockPoisoned"))?
            .iter()
            .find(|device| device.id == device_id)
            .cloned()
            .ok_or_else(|| AppError::new("error.connection.deviceOffline"))
    }

    /// 返回当前在线设备的最新基地址，供图片等设备业务统一复用。
    pub(super) fn current_device_base_url(&self) -> Result<String, AppError> {
        normalize_base_url(&self.current_online_device()?.base_url)
    }

    /// 用调用开始时取得的设备 ID 作为并发预条件。它不指定写入目标；实际目标
    /// 仍由当前 Rust 状态决定，选择已变化时拒绝跨设备执行。
    pub(super) fn current_device_base_url_for(
        &self,
        expected_device_id: &str,
    ) -> Result<String, AppError> {
        // expected ID 校验也必须位于同一事务内，不能在取出设备后先释放锁。
        let _snapshot_transaction = self.lock_device_snapshot_transaction()?;
        let device = self.current_online_device_locked()?;
        if device.id != expected_device_id {
            return Err(AppError::new("error.connection.deviceChanged"));
        }
        normalize_base_url(&device.base_url)
    }

    // 检测某个 base_url（未指定时用当前在线设备的最新地址）是否可连接，
    // 返回带提示信息的连接状态。
    pub async fn check_connection(
        &self,
        base_url: Option<&str>,
    ) -> Result<ConnectionStatus, AppError> {
        let base_url = match base_url {
            // 显式传入 url 时先做归一化校验。
            Some(value) => normalize_base_url(value)?,
            // 未传入则使用在线快照中的最新地址；离线时不回退历史地址。
            None => self.current_device_base_url()?,
        };
        // 请求设备的 /health 接口，超时比发现探测更长（用户主动触发的检测）。
        let result = self
            .client
            .get(format!("{base_url}/health"))
            .timeout(CONNECTION_CHECK_TIMEOUT)
            .send()
            .await;

        Ok(match result {
            // HTTP 状态码成功：视为可达。
            Ok(response) if response.status().is_success() => ConnectionStatus {
                reachable: true,
                base_url,
                message: "设备连接正常".to_owned(),
            },
            // 有响应但状态码非成功：视为不可达，附带状态码。
            Ok(response) => {
                let status_code = response.status().as_u16();
                warn!(base_url = %base_url, status_code, "设备连接检测收到非成功状态码");
                ConnectionStatus {
                    reachable: false,
                    base_url,
                    message: format!("设备返回 HTTP {status_code}"),
                }
            }
            // 请求本身失败（网络错误等）：视为不可达，附带错误详情。
            Err(error) => {
                warn!(base_url = %base_url, %error, "设备连接检测失败");
                ConnectionStatus {
                    reachable: false,
                    base_url,
                    message: format!("无法连接设备：{error}"),
                }
            }
        })
    }

    // 依次探测一组候选地址，返回第一个可达的地址（找不到则 None）。
    pub(super) async fn first_reachable_url(&self, base_urls: &[String]) -> Option<String> {
        for base_url in base_urls {
            if self.is_reachable(base_url).await {
                return Some(base_url.clone());
            }
        }
        None
    }

    // 探测单个 base_url 的 /health 接口是否可达（使用更短的探测超时，适合批量扫描）。
    pub(super) async fn is_reachable(&self, base_url: &str) -> bool {
        matches!(
            self.client
                .get(format!("{base_url}/health"))
                .timeout(DISCOVERY_PROBE_TIMEOUT)
                .send()
                .await,
            Ok(response) if response.status().is_success()
        )
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Read, Write},
        net::{Ipv4Addr, TcpListener},
        path::PathBuf,
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

    fn device(id: &str, base_url: &str) -> DiscoveredMonitorDevice {
        DiscoveredMonitorDevice {
            id: id.to_owned(),
            name: format!("Monitor {id}"),
            api_version: "1".to_owned(),
            base_url: base_url.to_owned(),
            path: "/api/device".to_owned(),
            discovery_source: DiscoverySource::Mdns,
        }
    }

    #[test]
    fn current_device_uses_latest_online_address_and_rejects_offline_history() {
        let (service, root) = loaded_service("current-online-device");
        service
            .select_device(&device("screen-1", "http://192.168.50.10:8080"))
            .unwrap();
        *service.online_devices.write().unwrap() = vec![
            device("screen-2", "http://192.168.50.20:8080"),
            device("screen-1", "http://192.168.50.99:9090/"),
        ];

        let current = service.current_online_device().unwrap();
        assert_eq!(current.id, "screen-1");
        assert_eq!(current.base_url, "http://192.168.50.99:9090/");
        assert_eq!(
            service.current_device_base_url().unwrap(),
            "http://192.168.50.99:9090"
        );
        assert_eq!(
            service.current_device_base_url_for("screen-2"),
            Err(AppError::new("error.connection.deviceChanged"))
        );
        // 清空发现快照后，持久化的旧地址仍在，但设备业务必须拒绝请求。
        service.online_devices.write().unwrap().clear();
        assert_eq!(
            service.current_device_base_url(),
            Err(AppError::new("error.connection.deviceOffline"))
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_connection_url_remains_available_without_a_selected_online_device() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let length = stream.read(&mut request).unwrap();
            assert!(String::from_utf8_lossy(&request[..length]).starts_with("GET /health "));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });
        let (service, root) = loaded_service("explicit-connection-url");
        let base_url = format!("http://{address}/");

        let status =
            tauri::async_runtime::block_on(service.check_connection(Some(&base_url))).unwrap();

        assert!(status.reachable);
        assert_eq!(status.base_url, format!("http://{address}"));
        server.join().unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
