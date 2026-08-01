use serde::{Deserialize, Serialize};

use super::device::ImageUploadAccept;

/// `AIMonitor` 设备原生保存并返回给桌面端的图片格式。
///
/// BMP 与 WebP 只属于上传输入格式：Rust 会在上传前把它们转换成设备支持的
/// PNG 或 GIF，因此它们不会出现在远端图片列表里。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImageFormat {
    Jpeg,
    Png,
    Gif,
}

impl ImageFormat {
    /// 从设备响应的 MIME 类型解析出稳定格式标识。
    pub(crate) fn from_mime_type(mime_type: &str) -> Option<Self> {
        match mime_type {
            "image/jpeg" => Some(Self::Jpeg),
            "image/png" => Some(Self::Png),
            "image/gif" => Some(Self::Gif),
            _ => None,
        }
    }

    /// 生成 data URL 和远端请求时使用的规范 MIME 类型。
    pub(crate) const fn mime_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Gif => "image/gif",
        }
    }
}

/// 上传处理器内部使用的输入格式。该枚举与下方描述表是一一对应关系。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UploadImageFormat {
    Bmp,
    Jpeg,
    Gif,
    Png,
    WebP,
}

struct UploadImageFormatDescriptor {
    format: UploadImageFormat,
    mime_types: &'static [&'static str],
    extensions: &'static [&'static str],
}

/// 上传格式识别、服务端校验和前端 accept 能力都从这张表派生，避免各层维护
/// 彼此可能漂移的 MIME/扩展名清单。
const UPLOAD_IMAGE_FORMATS: &[UploadImageFormatDescriptor] = &[
    UploadImageFormatDescriptor {
        format: UploadImageFormat::Bmp,
        mime_types: &["image/bmp", "image/x-ms-bmp"],
        extensions: &[".bmp"],
    },
    UploadImageFormatDescriptor {
        format: UploadImageFormat::Jpeg,
        mime_types: &["image/jpeg"],
        extensions: &[".jpg", ".jpeg"],
    },
    UploadImageFormatDescriptor {
        format: UploadImageFormat::Gif,
        mime_types: &["image/gif"],
        extensions: &[".gif"],
    },
    UploadImageFormatDescriptor {
        format: UploadImageFormat::Png,
        mime_types: &["image/png"],
        extensions: &[".png"],
    },
    UploadImageFormatDescriptor {
        format: UploadImageFormat::WebP,
        mime_types: &["image/webp"],
        extensions: &[".webp"],
    },
];

pub(crate) fn upload_image_format(mime_type: &str) -> Option<UploadImageFormat> {
    UPLOAD_IMAGE_FORMATS
        .iter()
        .find(|descriptor| descriptor.mime_types.contains(&mime_type))
        .map(|descriptor| descriptor.format)
}

pub(crate) fn is_supported_upload_image_mime(mime_type: &str) -> bool {
    upload_image_format(mime_type).is_some()
}

pub(super) fn image_upload_accept() -> ImageUploadAccept {
    ImageUploadAccept {
        mime_types: UPLOAD_IMAGE_FORMATS
            .iter()
            .flat_map(|descriptor| descriptor.mime_types.iter().copied())
            .map(str::to_owned)
            .collect(),
        extensions: UPLOAD_IMAGE_FORMATS
            .iter()
            .flat_map(|descriptor| descriptor.extensions.iter().copied())
            .map(str::to_owned)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{ImageFormat, UploadImageFormat, image_upload_accept, upload_image_format};

    #[test]
    fn remote_image_formats_have_a_stable_camel_case_contract() {
        assert_eq!(
            serde_json::to_string(&ImageFormat::Jpeg).unwrap(),
            r#""jpeg""#
        );
        assert_eq!(
            serde_json::to_string(&ImageFormat::Png).unwrap(),
            r#""png""#
        );
        assert_eq!(
            serde_json::to_string(&ImageFormat::Gif).unwrap(),
            r#""gif""#
        );
    }

    #[test]
    fn upload_accept_and_validation_share_the_same_format_table() {
        let accept = image_upload_accept();

        assert_eq!(
            accept.mime_types,
            [
                "image/bmp",
                "image/x-ms-bmp",
                "image/jpeg",
                "image/gif",
                "image/png",
                "image/webp",
            ]
        );
        assert_eq!(
            accept.extensions,
            [".bmp", ".jpg", ".jpeg", ".gif", ".png", ".webp"]
        );
        for mime_type in &accept.mime_types {
            assert!(upload_image_format(mime_type).is_some());
        }
        assert_eq!(
            upload_image_format("image/x-ms-bmp"),
            Some(UploadImageFormat::Bmp)
        );
        assert_eq!(upload_image_format("image/tiff"), None);
    }
}
