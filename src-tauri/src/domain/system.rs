// 引入 serde 的 Serialize，用于将结构体序列化为 JSON 返回给前端
use serde::Serialize;

// 派生 Serialize，使该结构体可以被序列化
#[derive(Serialize)]
// 序列化时字段名转换为 camelCase，以匹配 TypeScript 端的命名习惯
#[serde(rename_all = "camelCase")]
// 系统概览信息结构体
pub struct SystemOverview {
    // 应用名称
    app_name: &'static str,
    // 应用版本号
    version: &'static str,
    // 后端类型标识（固定为 "Rust"）
    backend: &'static str,
    // 前后端通信方式标识（固定为 "Tauri invoke"）
    transport: &'static str,
}

/// Produces application-owned system information.
///
/// This small function is the reference shape for future business use cases:
/// commands adapt transport concerns, while decisions and data composition stay
/// in the Rust domain layer.
// 标记：若返回值未被使用，编译器应给出警告提示
#[must_use]
// const fn：可在编译期求值的纯函数，构造并返回系统概览信息
pub const fn overview() -> SystemOverview {
    SystemOverview {
        // 从 Cargo 编译期环境变量读取包名
        app_name: env!("CARGO_PKG_NAME"),
        // 从 Cargo 编译期环境变量读取包版本号
        version: env!("CARGO_PKG_VERSION"),
        // 固定标注后端实现语言为 Rust
        backend: "Rust",
        // 固定标注前后端通信方式为 Tauri 的 invoke 机制
        transport: "Tauri invoke",
    }
}

// 仅在测试编译时包含以下测试模块
#[cfg(test)]
mod tests {
    // 引入待测试的 overview 函数
    use super::overview;

    // 测试用例：验证概览信息中正确报告了原生边界（后端与通信方式）
    #[test]
    fn overview_reports_the_native_boundary() {
        // 调用被测函数，获取概览结果
        let overview = overview();

        // 断言后端字段为 "Rust"
        assert_eq!(overview.backend, "Rust");
        // 断言通信方式字段为 "Tauri invoke"
        assert_eq!(overview.transport, "Tauri invoke");
    }
}
