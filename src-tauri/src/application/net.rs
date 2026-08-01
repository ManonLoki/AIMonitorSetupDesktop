// 本机/局域网 HTTP 客户端统一策略：关闭协议级隐式重试、重定向与系统代理。
// 中继与设备通信各自都有显式的重试/路由契约，不能被 reqwest 默认行为悄悄
// 改写；所有构造 reqwest 客户端的地方复用这两个函数而不是各自重复三行。
pub(crate) fn harden_async_client(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    builder
        .retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
}

pub(crate) fn harden_blocking_client(
    builder: reqwest::blocking::ClientBuilder,
) -> reqwest::blocking::ClientBuilder {
    builder
        .retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
}
