// 设备连接健康检查：主动检测（用户触发，超时更宽松）与批量发现时的
// 快速可达性探测（超时更短，供 mDNS/UDP 候选逐个探测使用）。
use std::time::Duration;

use serde::Serialize;

use super::{DISCOVERY_PROBE_TIMEOUT, MonitorService};
use crate::domain::monitor::normalize_base_url;

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
    // 检测某个 base_url（未指定时用当前保存设置）的设备是否可连接，返回带提示信息的连接状态。
    pub async fn check_connection(
        &self,
        base_url: Option<&str>,
    ) -> Result<ConnectionStatus, String> {
        let base_url = match base_url {
            // 显式传入 url 时先做归一化校验。
            Some(value) => normalize_base_url(value)?,
            // 未传入则使用当前保存的设备地址。
            None => self.settings()?.base_url,
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
            Ok(response) => ConnectionStatus {
                reachable: false,
                base_url,
                message: format!("设备返回 HTTP {}", response.status().as_u16()),
            },
            // 请求本身失败（网络错误等）：视为不可达，附带错误详情。
            Err(error) => ConnectionStatus {
                reachable: false,
                base_url,
                message: format!("无法连接设备：{error}"),
            },
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
