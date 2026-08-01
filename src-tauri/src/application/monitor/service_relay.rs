// 启动/查询本机 Hook 中继 HTTP 服务：axum 服务本身见 `relay::listener`，
// 状态机 worker 见 `relay::worker`；本文件只负责组装、启动与状态查询。
use std::sync::Arc;

use axum::{Router, extract::DefaultBodyLimit, routing::post};
use tokio::sync::mpsc;

use super::{
    HOOK_BIND_ADDRESS, HOOK_EVENT_QUEUE_CAPACITY, HOOK_LISTENER_PORT, HookListenerState,
    HookRelayStatus, IncomingHookEvent, MAX_HOOK_BODY_BYTES, MonitorService,
    build_hook_forward_client,
    relay::{handle_hook_request, record_relay_failure, spawn_hook_worker},
};

impl MonitorService {
    // 启动本机 Hook 中继：用 axum 起一个只绑定回环地址的 HTTP 服务接收本地 Hook 请求，
    // 再启动一个 Tokio task 从通道里取出请求做去抖判断并转发给设备。
    pub fn start_hook_listener(&self) {
        let data = Arc::clone(&self.data);
        let online_devices = Arc::clone(&self.online_devices);
        let status = Arc::clone(&self.relay_status);
        let device_snapshot_changes = self.hook_device_snapshot_revision.subscribe();

        // 构造用于向设备转发请求的阻塞式 HTTP 客户端。
        let client = match build_hook_forward_client() {
            Ok(client) => client,
            Err(error) => {
                record_relay_failure(&status, format!("无法创建转发客户端：{error}"));
                return;
            }
        };
        // 建立一个 Tokio 有界 mpsc 通道：axum handler 只负责接收、解析并交给状态机 worker。
        // 状态推进与设备网络投递已拆成两个阶段，worker 不会被慢设备阻塞；容量
        // 仍设为固定值，handler 通过 try_send 在洪峰时立即拒绝，既限制内存也不阻塞 runtime。
        let (sender, receiver) = mpsc::channel::<IncomingHookEvent>(HOOK_EVENT_QUEUE_CAPACITY);
        spawn_hook_worker(
            &client,
            receiver,
            device_snapshot_changes,
            &data,
            &online_devices,
            Arc::clone(&status),
        );

        // axum 服务运行在 Tauri 自带的 tokio 运行时上，无需为它单独起一个 runtime。
        let listener_state = HookListenerState {
            sender,
            status: Arc::clone(&status),
        };
        tauri::async_runtime::spawn(async move {
            // 绑定本地监听端口，失败则记录错误并直接结束（不重试）。
            let listener = match tokio::net::TcpListener::bind((
                HOOK_BIND_ADDRESS,
                HOOK_LISTENER_PORT,
            ))
            .await
            {
                Ok(listener) => listener,
                Err(error) => {
                    record_relay_failure(
                        &listener_state.status,
                        format!("Hook 监听端口启动失败：{error}"),
                    );
                    return;
                }
            };
            // 绑定成功后更新状态为“正在监听”，并清空历史错误信息。
            if let Ok(mut current) = listener_state.status.write() {
                current.listening = true;
                current.last_error.clear();
            }
            let app = Router::new()
                .route("/api/hooks/{tool_slug}", post(handle_hook_request))
                // 请求体超过限制时由 axum 直接拒绝（413），不进入业务解析逻辑。
                .layer(DefaultBodyLimit::max(MAX_HOOK_BODY_BYTES))
                .with_state(listener_state.clone());
            if let Err(error) = axum::serve(listener, app).await {
                record_relay_failure(
                    &listener_state.status,
                    format!("Hook 监听服务异常退出：{error}"),
                );
            }
            // 服务退出（理论上不会正常发生）后，把状态标记为未监听。
            if let Ok(mut current) = listener_state.status.write() {
                current.listening = false;
            }
        });
    }

    // 读取当前 Hook 中继状态快照，供前端查询展示。
    pub fn hook_relay_status(&self) -> Result<HookRelayStatus, String> {
        self.relay_status
            .read()
            .map(|status| status.clone())
            .map_err(|_| "Hook 服务状态读取锁已损坏".to_owned())
    }
}
