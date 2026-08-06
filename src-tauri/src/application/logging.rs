//! 应用日志初始化：基于 tracing，将日志写入 `app_data_dir/logs` 下按天滚动的文件；
//! debug 构建额外输出到标准错误，便于本地开发时直接查看。

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt};

const LOG_DIRECTORY_NAME: &str = "logs";
const LOG_FILE_PREFIX: &str = "aimonitor";
// 未设置 `RUST_LOG` 时的默认日志级别。
const DEFAULT_LOG_FILTER: &str = "info";

fn env_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER))
}

/// 初始化全局日志订阅者。返回的 `WorkerGuard` 必须存活到进程退出（调用方应将其
/// 交给 `Tauri` 状态管理持有），否则非阻塞写入线程会提前关闭，导致缓冲中的日志丢失。
pub fn init(app_data_dir: &Path) -> WorkerGuard {
    let log_dir = app_data_dir.join(LOG_DIRECTORY_NAME);
    let file_appender = tracing_appender::rolling::daily(log_dir, LOG_FILE_PREFIX);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_filter(env_filter());

    // debug 构建下额外附加一层标准错误输出；release 构建只写日志文件，
    // 避免托盘常驻应用在无控制台的环境下产生无意义的标准错误开销。
    let stderr_layer = cfg!(debug_assertions).then(|| {
        fmt::layer()
            .with_writer(std::io::stderr)
            .with_filter(env_filter())
    });

    tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer)
        .init();

    guard
}
