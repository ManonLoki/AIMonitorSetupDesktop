// 本机 Hook 中继：接收（`listener`）、状态机推进与设备投递编排（`worker`）、
// 实际的设备 HTTP 转发（`forward`）、中继状态记账（`status`）。
// `service_relay.rs`（`application::monitor` 的兄弟子模块）只负责组装启动。
mod forward;
mod listener;
mod status;
mod worker;

pub(crate) use listener::handle_hook_request;
pub(crate) use status::record_relay_failure;
pub(crate) use worker::spawn_hook_worker;
