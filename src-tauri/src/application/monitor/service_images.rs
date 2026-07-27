// 设备端图片相关接口的编排：拉取远端图片列表/内容、批量上传（含压缩转换）、
// 删除。成功性校验/错误信息提取复用 `device_response` 模块。
use reqwest::{Url, header, multipart};
use serde::{Deserialize, Serialize};

use super::{MAX_REMOTE_IMAGE_BYTES, MonitorService, device_response::ensure_success};
use crate::domain::monitor::{encode_base64, process_image_upload};

// 从设备下载的远程图片，image 字段为 base64 编码内容。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteImage {
    pub filename: String,
    pub mime_type: String,
    pub image: String,
}

// 设备端图片列表接口的响应体。
#[derive(Deserialize)]
struct ImageListResponse {
    images: Vec<RemoteImageMetadata>,
}

// 图片列表中单条图片的元数据（文件名与 MIME 类型）。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteImageMetadata {
    filename: String,
    mime_type: String,
}

// 图片上传接口成功后返回的响应体，携带最终保存的文件名。
#[derive(Deserialize)]
struct UploadResponse {
    filename: String,
}

// 从前端接收的待上传图片：文件名、MIME 类型与原始字节内容。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageUpload {
    filename: String,
    mime_type: String,
    bytes: Vec<u8>,
}

// 拼出下载单张图片的完整 URL，同时校验文件名合法，防止路径穿越攻击。
fn remote_image_url(base_url: &str, filename: &str) -> Result<Url, String> {
    // 拒绝空文件名、"." "/.." 以及包含路径分隔符的文件名。
    if filename.is_empty() || filename == "." || filename == ".." || filename.contains(['/', '\\'])
    {
        return Err("远端图片文件名无效".to_owned());
    }

    let mut url = Url::parse(&format!("{base_url}/api/images/"))
        .map_err(|error| format!("设备图片地址无效：{error}"))?;
    // 通过 path_segments_mut 安全地追加文件名段（会自动做 URL 编码），
    // 而不是用字符串拼接，避免特殊字符导致的问题。
    url.path_segments_mut()
        .map_err(|()| "设备图片地址不能包含路径段".to_owned())?
        .pop_if_empty()
        .push(filename);
    Ok(url)
}

// 判断设备端返回的 MIME 类型是否属于其原生支持的图片格式。
fn is_supported_image_mime(mime_type: &str) -> bool {
    matches!(mime_type, "image/jpeg" | "image/png" | "image/gif")
}

// 判断桌面端可以接收并在上传前处理的图片 MIME 类型。
fn is_supported_upload_image_mime(mime_type: &str) -> bool {
    matches!(
        mime_type,
        "image/bmp" | "image/x-ms-bmp" | "image/jpeg" | "image/png" | "image/gif" | "image/webp"
    )
}

// 校验图片字节长度不超过最大限制。
fn ensure_image_size(len: u64, filename: &str) -> Result<(), String> {
    if len > MAX_REMOTE_IMAGE_BYTES as u64 {
        return Err(format!("{filename} 不能超过 8 MiB"));
    }
    Ok(())
}

// 上传前的批量校验：必须至少选择一张图片，且每张图片文件名非空、内容非空、
// 大小不超限、MIME 类型受支持；任意一张不满足都直接整体拒绝（不做部分上传）。
fn validate_image_uploads(images: &[ImageUpload]) -> Result<(), String> {
    if images.is_empty() {
        return Err("请选择要上传的图片".to_owned());
    }

    for image in images {
        if image.filename.trim().is_empty() || image.bytes.is_empty() {
            return Err("所选图片中包含空文件".to_owned());
        }
        ensure_image_size(image.bytes.len() as u64, &image.filename)?;
        if !is_supported_upload_image_mime(&image.mime_type) {
            return Err(format!(
                "{} 不是支持的 BMP、JPEG、GIF、PNG 或 WebP 图片",
                image.filename
            ));
        }
    }

    Ok(())
}

impl MonitorService {
    // 获取设备端保存的所有图片：先拉取图片列表元数据，再逐张下载图片内容。
    pub async fn images(&self) -> Result<Vec<RemoteImage>, String> {
        let base_url = self.settings()?.base_url;
        let response = self
            .client
            .get(format!("{base_url}/api/images"))
            .send()
            .await
            .map_err(|error| format!("无法读取远端图片：{error}"))?;
        // 校验 HTTP 状态码，非成功状态转换为业务错误。
        let response = ensure_success(response).await?;
        let metadata = response
            .json::<ImageListResponse>()
            .await
            .map(|body| body.images)
            .map_err(|error| format!("图片列表响应格式错误：{error}"))?;
        let mut images = Vec::with_capacity(metadata.len());

        // AIMonitor serves image bytes through GET /api/images/{filename}.
        // Fetch sequentially because the embedded device handles only a small
        // number of concurrent connections reliably.
        for item in metadata {
            images.push(self.remote_image(&base_url, item).await?);
        }

        Ok(images)
    }

    // 下载单张远端图片并转换为 base64 data url，同时做 MIME 类型和大小校验。
    async fn remote_image(
        &self,
        base_url: &str,
        metadata: RemoteImageMetadata,
    ) -> Result<RemoteImage, String> {
        let filename = metadata.filename.trim();
        // 拼出图片下载 url（内部会对文件名做安全校验，防止路径穿越）。
        let url = remote_image_url(base_url, filename)?;
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| format!("{filename} 读取失败：{error}"))?;
        let response = ensure_success(response).await?;
        // 若响应头带有 Content-Length，提前校验大小是否超限，避免读取超大响应体。
        if let Some(length) = response.content_length() {
            ensure_image_size(length, filename)?;
        }

        // 优先信任响应头里的 Content-Type（去掉可能的 charset 等参数），
        // 找不到或不支持时回退使用元数据里记录的 MIME 类型。
        let header_mime = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::trim);
        let mime_type = [header_mime, Some(metadata.mime_type.trim())]
            .into_iter()
            .flatten()
            .find(|value| is_supported_image_mime(value))
            .ok_or_else(|| format!("{filename} 返回了不支持的图片类型"))?
            .to_owned();

        // 读取响应体全部字节。
        let bytes = response
            .bytes()
            .await
            .map_err(|error| format!("{filename} 读取失败：{error}"))?;
        // 读取到实际字节后再次校验大小（防止 Content-Length 缺失或不准确的情况）。
        ensure_image_size(bytes.len() as u64, filename)?;

        // 编码为 base64 data url，方便前端直接用作 img src。
        let image = format!("data:{mime_type};base64,{}", encode_base64(&bytes));
        Ok(RemoteImage {
            filename: filename.to_owned(),
            mime_type,
            image,
        })
    }

    // 批量上传图片到设备：先做业务校验，再逐张压缩后以 multipart 表单上传。
    pub async fn upload_images(&self, images: Vec<ImageUpload>) -> Result<Vec<String>, String> {
        validate_image_uploads(&images)?;
        let base_url = self.settings()?.base_url;
        let mut uploaded = Vec::with_capacity(images.len());

        for image in images {
            let source_filename = image.filename.clone();
            // 上传前在 Rust domain 层完成格式校验、缩放及兼容格式转换。
            let processed = process_image_upload(&source_filename, &image.bytes, &image.mime_type)
                .map_err(|error| format!("{source_filename} 处理失败：{error}"))?;
            ensure_image_size(processed.bytes.len() as u64, &processed.filename)?;
            // 构造 multipart 表单的文件分片。
            let file_part = multipart::Part::bytes(processed.bytes)
                .file_name(processed.filename.clone())
                .mime_str(processed.mime_type)
                .map_err(|error| format!("{source_filename} 的图片类型无效：{error}"))?;
            let response = self
                .client
                .post(format!("{base_url}/api/images"))
                .multipart(multipart::Form::new().part("file", file_part))
                .send()
                .await
                .map_err(|error| format!("{source_filename} 上传失败：{error}"))?;
            let response = ensure_success(response).await?;
            let uploaded_filename = response
                .json::<UploadResponse>()
                .await
                .map(|body| body.filename)
                .map_err(|error| format!("{source_filename} 的上传响应格式错误：{error}"))?;
            uploaded.push(uploaded_filename);
        }

        Ok(uploaded)
    }

    // 删除设备端的一张图片。
    pub async fn delete_image(&self, filename: &str) -> Result<(), String> {
        let base_url = self.settings()?.base_url;
        let response = self
            .client
            .delete(format!("{base_url}/api/images/{filename}"))
            .send()
            .await
            .map_err(|error| format!("删除图片失败：{error}"))?;
        ensure_success(response).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 验证批量图片上传校验会检查列表中的每一个文件，而不是遇到第一个合法文件就通过；
    // 列表里混入一个不支持的 TIFF 格式应导致整体校验失败。
    #[test]
    fn batch_image_validation_checks_every_file_before_upload() {
        let images = vec![
            ImageUpload {
                filename: "valid.png".to_owned(),
                mime_type: "image/png".to_owned(),
                bytes: vec![1],
            },
            ImageUpload {
                filename: "invalid.tiff".to_owned(),
                mime_type: "image/tiff".to_owned(),
                bytes: vec![1],
            },
        ];

        // 即使第一个文件合法，只要列表中有一个不支持的类型，整体也应报错。
        assert_eq!(
            validate_image_uploads(&images),
            Err("invalid.tiff 不是支持的 BMP、JPEG、GIF、PNG 或 WebP 图片".to_owned())
        );
    }

    #[test]
    fn batch_image_validation_accepts_all_supported_upload_types() {
        let images = [
            ("legacy.bmp", "image/bmp"),
            ("photo.jpg", "image/jpeg"),
            ("photo.jpeg", "image/jpeg"),
            ("moving.gif", "image/gif"),
            ("graphic.png", "image/png"),
            ("modern.webp", "image/webp"),
        ]
        .map(|(filename, mime_type)| ImageUpload {
            filename: filename.to_owned(),
            mime_type: mime_type.to_owned(),
            bytes: vec![1],
        });

        assert!(validate_image_uploads(&images).is_ok());
    }

    // 验证空的图片选择会被拒绝，提示用户先选择图片。
    #[test]
    fn batch_image_validation_rejects_an_empty_selection() {
        assert_eq!(
            validate_image_uploads(&[]),
            Err("请选择要上传的图片".to_owned())
        );
    }

    // 验证 remote_image_url 会对文件名做正确的 URL 编码（含中文、空格、# 等特殊字符），
    // 同时验证包含路径穿越（"../secret"）的文件名会被拒绝。
    #[test]
    fn remote_image_url_encodes_one_filename_path_segment() {
        let url = remote_image_url("http://192.168.50.20:8080", "状态 图片 #1.gif").unwrap();

        assert_eq!(
            url.as_str(),
            "http://192.168.50.20:8080/api/images/%E7%8A%B6%E6%80%81%20%E5%9B%BE%E7%89%87%20%231.gif"
        );
        assert!(remote_image_url("http://192.168.50.20:8080", "../secret").is_err());
    }
}
