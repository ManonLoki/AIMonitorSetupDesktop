// 设备 HTTP 响应的成功性校验，阻塞版（Hook 转发用）与异步版（图片/连接类
// 接口用）共用同一套错误信息提取逻辑：优先使用设备返回的 JSON 错误正文，
// 解析不到（含 204 无正文）时退回 HTTP 状态码文案。
use reqwest::StatusCode;
use serde::Deserialize;

// 设备端错误响应体的通用结构，仅包含一条错误信息字符串。
#[derive(Deserialize)]
pub(super) struct ErrorResponse {
    error: String,
}

/// 把设备的非 2xx 响应转成对用户有意义的错误：优先使用设备返回的
/// `ErrorResponse.error`，解析不到（包括 204 无正文的情况）时退回到
/// HTTP 状态码文案。供阻塞版和异步版 `ensure_success*` 共用。
pub(super) fn device_error_message(
    status: StatusCode,
    parsed_body: Option<ErrorResponse>,
) -> String {
    // 优先使用设备返回的错误正文；解析不到（比如 204 无正文）则退回状态码文案。
    parsed_body.map_or_else(
        || format!("设备请求失败（HTTP {}）", status.as_u16()),
        |body| body.error,
    )
}

// 阻塞版的响应成功性校验：非 2xx 时尝试解析错误正文并转换为业务错误。
pub(super) fn ensure_success_blocking(
    response: reqwest::blocking::Response,
) -> Result<reqwest::blocking::Response, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    // 204 No Content 没有正文可解析，直接跳过解析步骤。
    let parsed_body = if status == StatusCode::NO_CONTENT {
        None
    } else {
        response.json::<ErrorResponse>().ok()
    };
    Err(device_error_message(status, parsed_body))
}

// 异步版的响应成功性校验（对应阻塞版 ensure_success_blocking），逻辑相同。
pub(super) async fn ensure_success(
    response: reqwest::Response,
) -> Result<reqwest::Response, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let parsed_body = if status == StatusCode::NO_CONTENT {
        None
    } else {
        response.json::<ErrorResponse>().await.ok()
    };
    Err(device_error_message(status, parsed_body))
}
