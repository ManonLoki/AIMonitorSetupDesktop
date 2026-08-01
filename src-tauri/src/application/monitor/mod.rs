// 监控设备应用服务：`MonitorService` 持有所有共享状态句柄（数据、在线设备快照、
// Hook 中继状态等），具体编排逻辑按关注点拆分到下列子模块中。
// 本模块（application 层）只做编排，具体业务规则在 domain 层实现。
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use reqwest::Client;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::domain::monitor::{
    AiTool, DEFAULT_HOOK_RELAY_PORT, DiscoveredMonitorDevice, HookBehavior, HookConfigDirectories,
    SavedMonitorData,
};

mod config_io;
mod connection;
mod device_response;
mod discovery;
mod heartbeat;
mod hook_config;
mod relay;
mod service_discovery;
mod service_hooks;
mod service_images;
mod service_lifecycle;
mod service_relay;
mod wsl;

#[cfg(test)]
mod test_support;

pub use connection::ConnectionStatus;
pub use service_discovery::MonitorDeviceSnapshot;
pub use service_images::{ImageUpload, RemoteImageGallery};

// 本地持久化存储文件名（保存监控配置数据的 JSON 文件）。
const STORE_FILENAME: &str = "monitor-data.json";
// mDNS 服务发现使用的服务类型标识。
const AIMONITOR_SERVICE_TYPE: &str = "_aimonitor._tcp.local.";
// 整体发现流程的超时时间。
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
// 对单个候选地址做连接探测时的超时时间。
const DISCOVERY_PROBE_TIMEOUT: Duration = Duration::from_millis(900);
// UDP 广播发现所使用的端口。
const UDP_DISCOVERY_PORT: u16 = 8080;
// UDP 广播发现请求的固定报文内容。
const UDP_DISCOVERY_REQUEST: &[u8] = b"AIMONITOR_DISCOVER_V1";
// 等待 UDP 广播响应的超时时间。
const UDP_DISCOVERY_TIMEOUT: Duration = Duration::from_millis(1_200);
// 单个 UDP 响应报文允许的最大字节数，超出则丢弃。
const UDP_RESPONSE_MAX_BYTES: usize = 1_024;
// 设备 HTTP API 的默认路径前缀。
const DEFAULT_DEVICE_API_PATH: &str = "/api/device";
// 允许下载的远程图片最大字节数（8MB），超过则拒绝。
const MAX_REMOTE_IMAGE_BYTES: usize = 8 * 1024 * 1024;
// 本机 Hook 中继监听端口，直接复用领域层的默认端口常量。
pub const HOOK_LISTENER_PORT: u16 = DEFAULT_HOOK_RELAY_PORT;
// Hook 中继 HTTP 服务只绑定在本机回环地址，不对外暴露。
const HOOK_BIND_ADDRESS: &str = "127.0.0.1";
// 向设备转发 Hook 状态所用客户端的连接/请求超时：设备均在局域网内，
// 2 秒足以覆盖正常网络抖动，同时避免慢设备长时间占用投递线程。
const HOOK_FORWARD_CLIENT_TIMEOUT: Duration = Duration::from_secs(2);
// 控制端每 30 秒向当前在线且已配置 Profile 的接收端续租一次。
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
// listener 只接受 relay 子进程/原生插件生成的最小 Hook 信封；4 KiB 足以容纳
// 事件名、会话/轮次 ID 和状态，同时从接口边界拒绝 prompt、工具输出等大载荷。
const MAX_HOOK_BODY_BYTES: usize = 4 * 1024;
// listener 到状态机 worker 的队列使用固定容量；状态机推进不再等待设备网络，
// 正常情况下会很快腾出空间，极端洪峰则通过短暂背压保护进程内存。
const HOOK_EVENT_QUEUE_CAPACITY: usize = 256;
// 每个“设备 + 工具”投递 worker 最多只需要一个唤醒令牌：若目标队列已有
// 待发送状态，后续事件按该工具的 latest-wins/FIFO 策略更新队列即可。
const HOOK_RELAY_WAKE_QUEUE_CAPACITY: usize = 1;
// 会话长时间没有任何事件时视为孤儿并回收。超时只清理内部记录，不释放设备槽位；
// 新事件仍可按当前事件语义重新建立隐式会话，因此不会让后续真实状态永久丢失。
const HOOK_SESSION_INACTIVITY_TIMEOUT: Duration = Duration::from_mins(30);
// 即使没有新 Hook，也按此粒度清扫一次超时会话。
const HOOK_SESSION_SWEEP_INTERVAL: Duration = Duration::from_secs(1);
// 会进入队列或状态机长期保存的上下文字段使用独立上限。
const MAX_HOOK_SESSION_ID_BYTES: usize = 512;
const MAX_HOOK_TURN_ID_BYTES: usize = 512;
const MAX_HOOK_STATUS_BYTES: usize = 64;
// 连续多少次发现轮询未命中某设备后，才将其判定为离线并移除。
const DISCOVERY_MISSES_BEFORE_REMOVAL: u8 = 2;
/// 后台发现循环的轮询粒度：每次醒来都会重新读取当前配置的检查间隔，
/// 因此设置页修改间隔后，最多这么久就会生效，无需重启线程。
const DISCOVERY_POLL_GRANULARITY: Duration = Duration::from_secs(1);
// 设备列表发生变化时向前端发送的 Tauri 事件名称。
pub const MONITOR_DEVICES_CHANGED_EVENT: &str = "monitor-devices-changed";

// 构造用于向设备转发 Hook 状态/心跳的阻塞式 HTTP 客户端。中继监听与心跳
// 发送两条独立后台线程共用同一套连接/请求超时配置，因此只在此处实现一次。
fn build_hook_forward_client() -> reqwest::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .connect_timeout(HOOK_FORWARD_CLIENT_TIMEOUT)
        .timeout(HOOK_FORWARD_CLIENT_TIMEOUT)
        // 状态投递必须严格保持“一次转换只发送一次”，不继承 reqwest 的协议级重试。
        .retry(reqwest::retry::never())
        // 在线目标不能通过 3xx 把状态请求转移到发现快照之外的地址。
        .redirect(reqwest::redirect::Policy::none())
        // 环回与局域网设备通信不经过系统代理。
        .no_proxy()
        .build()
}

// 构造监控设备业务使用的异步 HTTP 客户端。调用方仍可按具体操作设置请求级
// timeout，但所有设备请求统一禁用隐藏重试、重定向和系统代理。
fn build_monitor_client() -> Result<Client, String> {
    Client::builder()
        .retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|error| format!("无法创建监控设备 HTTP 客户端：{error}"))
}

// 应用核心服务：持有 HTTP 客户端、数据存储路径与内存态、在线设备快照、
// 发现去抖计数、Hook 配置写入互斥锁、Hook 中继状态等所有共享状态。
// Clone 只克隆 Arc/Client 句柄，底层状态仍然共享。
#[derive(Default)]
struct DeviceSnapshotState {
    // 已提交设备快照的进程内单调修订号。
    revision: u64,
    // 最近发起的发现世代；较旧扫描晚完成时必须放弃提交。
    latest_refresh_generation: u64,
}

#[derive(Clone)]
pub struct MonitorService {
    // 复用的 reqwest HTTP 客户端。
    client: Client,
    // 本地持久化数据文件路径。
    data_path: PathBuf,
    // 默认探测到的各 AI 工具 Hook 配置目录。
    default_hook_config_directories: HookConfigDirectories,
    // 内存中的已保存监控数据（读写锁保护）。
    data: Arc<RwLock<SavedMonitorData>>,
    // 当前在线设备快照（后台轮询线程更新）。
    online_devices: Arc<RwLock<Vec<DiscoveredMonitorDevice>>>,
    // 各设备连续未被发现命中的次数，用于去抖判定离线。
    discovery_missed_scans: Arc<Mutex<HashMap<String, u8>>>,
    // 设备快照事务锁兼进程内单调 revision。发现提交与手动切换共用
    // 这一个临界区，防止 settings/online_devices 被交错组合成撕裂快照。
    device_snapshot_state: Arc<Mutex<DeviceSnapshotState>>,
    // Hook 配置文件写入互斥锁，避免并发写入导致文件损坏。
    hook_config_write_lock: Arc<Mutex<()>>,
    // Hook 中继监听/转发状态（供前端查询展示）。
    relay_status: Arc<RwLock<HookRelayStatus>>,
}

// Hook 中继状态快照，序列化后暴露给前端展示中继运行情况。
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRelayStatus {
    // 中继 HTTP 服务是否正在监听。
    pub listening: bool,
    // 监听绑定的地址。
    pub bind_address: String,
    // 监听端口。
    pub port: u16,
    // 累计收到的 Hook 请求数。
    pub received_count: u64,
    // 累计成功转发的次数。
    pub forwarded_count: u64,
    // 累计转发失败的次数。
    pub failed_count: u64,
    // 累计被抑制（忽略）未转发的次数。
    pub suppressed_count: u64,
    // 当前排队等待处理的数量。
    pub pending_count: u64,
    // 最近一次事件的显式处理结果；前端无需从空 behavior 推断“释放”。
    pub last_event: Option<HookRelayLastEvent>,
    // 最近一次的错误信息（无错误时为空字符串）。
    pub last_error: String,
}

/// 最近一次 Hook 的业务结果。`kind` 是稳定判别字段，展示行为、释放和时序
/// 抑制互斥，避免跨字段组合产生歧义。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HookRelayLastEvent {
    Display {
        tool: AiTool,
        #[serde(rename = "hookType")]
        hook_type: String,
        behavior: HookBehavior,
    },
    Release {
        tool: AiTool,
        #[serde(rename = "hookType")]
        hook_type: String,
    },
    Suppressed {
        tool: AiTool,
        #[serde(rename = "hookType")]
        hook_type: String,
    },
}

// axum handler（`relay::listener`）解析出的一个 Hook 事件；由状态机 worker
// （`relay::worker`）消费。两者都是本模块的子模块，因此保持 private 即可。
#[derive(Debug, PartialEq, Eq)]
struct IncomingHookEvent {
    tool: AiTool,
    hook_type: String,
    session_id: Option<String>,
    turn_id: Option<String>,
    status: Option<String>,
}

// axum handler 共享的状态：解析成功后把事件送进状态机 worker 的发送端，
// 以及供 handler 记账/记录失败信息的中继状态句柄。
#[derive(Clone)]
struct HookListenerState {
    sender: mpsc::Sender<IncomingHookEvent>,
    status: Arc<RwLock<HookRelayStatus>>,
}
