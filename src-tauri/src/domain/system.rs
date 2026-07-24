use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemOverview {
    app_name: &'static str,
    version: &'static str,
    backend: &'static str,
    transport: &'static str,
}

/// Produces application-owned system information.
///
/// This small function is the reference shape for future business use cases:
/// commands adapt transport concerns, while decisions and data composition stay
/// in the Rust domain layer.
#[must_use]
pub const fn overview() -> SystemOverview {
    SystemOverview {
        app_name: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        backend: "Rust",
        transport: "Tauri invoke",
    }
}

#[cfg(test)]
mod tests {
    use super::overview;

    #[test]
    fn overview_reports_the_native_boundary() {
        let overview = overview();

        assert_eq!(overview.backend, "Rust");
        assert_eq!(overview.transport, "Tauri invoke");
    }
}
