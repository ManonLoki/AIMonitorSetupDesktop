/// 长边超过该像素数的上传图片会被等比缩小到该尺寸以内。
pub const MAX_UPLOAD_IMAGE_EDGE: u32 = 800;
// 重新编码 JPEG 时使用的固定质量参数（0-100，值越大质量越高、体积越大）。
const UPLOAD_JPEG_QUALITY: u8 = 82;

/// 上传前处理完成的图片。BMP 与 WebP 会转换为目标端兼容性更好的格式，因此
/// 文件名和 MIME 类型可能与用户选择的原文件不同。
pub struct ProcessedUploadImage {
    pub filename: String,
    pub mime_type: &'static str,
    pub bytes: Vec<u8>,
}

/// 处理待上传图片，降低展示屏（Android 端）解码大图的压力：
///
/// - JPEG/PNG 会在长边超过 [`MAX_UPLOAD_IMAGE_EDGE`] 时等比缩小并重新编码；
/// - GIF 校验格式后原样透传，避免丢失动画；
/// - BMP 转成 PNG；
/// - 静态 WebP 转成 PNG，动画 WebP 保留帧和延时并转成 GIF。
pub fn process_image_upload(
    filename: &str,
    bytes: &[u8],
    mime_type: &str,
) -> Result<ProcessedUploadImage, String> {
    match mime_type {
        "image/gif" => {
            image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes))
                .map_err(|error| format!("图片解码失败：{error}"))?;
            Ok(ProcessedUploadImage {
                filename: filename.to_owned(),
                mime_type: "image/gif",
                bytes: bytes.to_vec(),
            })
        }
        "image/webp" => process_webp_upload(filename, bytes),
        "image/jpeg" => process_static_upload(
            filename,
            bytes,
            image::ImageFormat::Jpeg,
            "image/jpeg",
            false,
        ),
        "image/png" => {
            process_static_upload(filename, bytes, image::ImageFormat::Png, "image/png", false)
        }
        "image/bmp" | "image/x-ms-bmp" => {
            process_static_upload(filename, bytes, image::ImageFormat::Bmp, "image/png", true)
        }
        _ => Err("不支持的图片类型".to_owned()),
    }
}

// 解码静态图片，超出最大边长时按比例缩放，`convert_to_png` 为真时
// 重新编码为 PNG（用于设备端原生不支持的格式），否则保留原格式重新编码。
fn process_static_upload(
    filename: &str,
    bytes: &[u8],
    source_format: image::ImageFormat,
    output_mime_type: &'static str,
    convert_to_png: bool,
) -> Result<ProcessedUploadImage, String> {
    let output_format = if convert_to_png {
        image::ImageFormat::Png
    } else {
        source_format
    };

    // 按声明的格式解码图片；解码失败说明数据损坏或格式不匹配。
    let decoded = image::load_from_memory_with_format(bytes, source_format)
        .map_err(|error| format!("图片解码失败：{error}"))?;
    // 任一边超过上限就需要缩放。
    let needs_resize =
        decoded.width() > MAX_UPLOAD_IMAGE_EDGE || decoded.height() > MAX_UPLOAD_IMAGE_EDGE;
    let image = if needs_resize {
        // 使用 Lanczos3 滤波器等比缩放，使长边不超过 MAX_UPLOAD_IMAGE_EDGE。
        decoded.resize(
            MAX_UPLOAD_IMAGE_EDGE,
            MAX_UPLOAD_IMAGE_EDGE,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        // 未超限则保持原尺寸，后面仍会重新编码一次尝试压缩体积。
        decoded
    };

    let mut output = Vec::new();
    match output_format {
        image::ImageFormat::Jpeg => {
            // 按固定质量参数重新编码为 JPEG。
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut output,
                UPLOAD_JPEG_QUALITY,
            );
            image
                .write_with_encoder(encoder)
                .map_err(|error| format!("图片压缩失败：{error}"))?;
        }
        image::ImageFormat::Png => {
            // PNG 使用最高压缩级别 + 自适应滤波器，尽量减小体积。
            let encoder = image::codecs::png::PngEncoder::new_with_quality(
                &mut output,
                image::codecs::png::CompressionType::Best,
                image::codecs::png::FilterType::Adaptive,
            );
            image
                .write_with_encoder(encoder)
                .map_err(|error| format!("图片压缩失败：{error}"))?;
        }
        // output_format 在调用方已被限定为 Jpeg 或 Png 之一。
        _ => unreachable!("输出格式只能是 Jpeg 或 Png"),
    }

    // 没有缩放且重新编码后体积没有变小，则保留原始字节，避免做无意义的替换。
    let output_bytes = if !convert_to_png && !needs_resize && output.len() >= bytes.len() {
        bytes.to_vec()
    } else {
        output
    };
    let output_filename = if convert_to_png {
        replace_image_extension(filename, "png")
    } else {
        filename.to_owned()
    };
    Ok(ProcessedUploadImage {
        filename: output_filename,
        mime_type: output_mime_type,
        bytes: output_bytes,
    })
}

// WebP 需要单独处理：静态图直接走通用缩放/转码路径；动图设备端不支持，
// 转成动图 GIF 保留动画效果（GIF 是设备原生支持的动图格式）。
fn process_webp_upload(filename: &str, bytes: &[u8]) -> Result<ProcessedUploadImage, String> {
    use image::AnimationDecoder as _;

    let decoder = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(bytes))
        .map_err(|error| format!("图片解码失败：{error}"))?;
    if !decoder.has_animation() {
        return process_static_upload(filename, bytes, image::ImageFormat::WebP, "image/png", true);
    }

    let repeat = match decoder.loop_count() {
        image::metadata::LoopCount::Infinite => image::codecs::gif::Repeat::Infinite,
        image::metadata::LoopCount::Finite(count) => {
            image::codecs::gif::Repeat::Finite(u16::try_from(count.get()).unwrap_or(u16::MAX))
        }
    };
    let frames = decoder
        .into_frames()
        .map(|frame| {
            let frame = frame?;
            let delay = frame.delay();
            let buffer = frame.into_buffer();
            let resized = if buffer.width() > MAX_UPLOAD_IMAGE_EDGE
                || buffer.height() > MAX_UPLOAD_IMAGE_EDGE
            {
                image::DynamicImage::ImageRgba8(buffer)
                    .resize(
                        MAX_UPLOAD_IMAGE_EDGE,
                        MAX_UPLOAD_IMAGE_EDGE,
                        image::imageops::FilterType::Lanczos3,
                    )
                    .to_rgba8()
            } else {
                buffer
            };
            Ok(image::Frame::from_parts(resized, 0, 0, delay))
        })
        .collect::<image::ImageResult<Vec<_>>>()
        .map_err(|error| format!("图片解码失败：{error}"))?;

    let mut output = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new_with_speed(&mut output, 10);
        encoder
            .set_repeat(repeat)
            .and_then(|()| encoder.encode_frames(frames))
            .map_err(|error| format!("WebP 动图转 GIF 失败：{error}"))?;
    }
    Ok(ProcessedUploadImage {
        filename: replace_image_extension(filename, "gif"),
        mime_type: "image/gif",
        bytes: output,
    })
}

// 格式转换后文件名后缀需要同步更新（如 .webp → .gif），否则设备端会按
// 旧后缀误判格式。
fn replace_image_extension(filename: &str, extension: &str) -> String {
    std::path::Path::new(filename)
        .with_extension(extension)
        .to_string_lossy()
        .into_owned()
}

// 仅在测试构建中编译的单元测试模块，覆盖本文件内的纯业务逻辑。
#[cfg(test)]
mod tests {
    use super::{MAX_UPLOAD_IMAGE_EDGE, process_image_upload};

    // 测试辅助函数：生成指定宽高、指定格式的纯色测试图片字节数据。
    fn encode_test_image(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
        let image = image::DynamicImage::new_rgb8(width, height);
        let mut bytes = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut bytes), format)
            .unwrap();
        bytes
    }

    // 验证超过上限尺寸的 JPEG 图片会被等比缩小，长边缩到 MAX_UPLOAD_IMAGE_EDGE，
    // 短边按原图宽高比例同步缩小。
    #[test]
    fn oversized_jpeg_upload_is_scaled_to_max_edge() {
        let source = encode_test_image(2000, 500, image::ImageFormat::Jpeg);

        let processed = process_image_upload("wide.jpg", &source, "image/jpeg").unwrap();

        let decoded =
            image::load_from_memory_with_format(&processed.bytes, image::ImageFormat::Jpeg)
                .unwrap();
        assert_eq!(decoded.width(), MAX_UPLOAD_IMAGE_EDGE);
        assert_eq!(decoded.height(), 200);
        assert_eq!(processed.filename, "wide.jpg");
        assert_eq!(processed.mime_type, "image/jpeg");
    }

    // 验证未超限的小尺寸 PNG 图片在处理后尺寸保持不变。
    #[test]
    fn small_png_upload_keeps_its_dimensions() {
        let source = encode_test_image(100, 50, image::ImageFormat::Png);

        let processed = process_image_upload("small.png", &source, "image/png").unwrap();

        let decoded =
            image::load_from_memory_with_format(&processed.bytes, image::ImageFormat::Png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (100, 50));
    }

    // 验证 GIF 图片原样透传，不做任何解码/重新编码（避免丢失动画帧）。
    #[test]
    fn gif_upload_passes_through_unchanged() {
        let source = encode_test_image(2, 2, image::ImageFormat::Gif);

        let processed = process_image_upload("moving.gif", &source, "image/gif").unwrap();

        assert_eq!(processed.bytes, source);
        assert_eq!(processed.filename, "moving.gif");
        assert_eq!(processed.mime_type, "image/gif");
    }

    #[test]
    fn bmp_upload_is_converted_to_png() {
        let source = encode_test_image(16, 8, image::ImageFormat::Bmp);

        let processed = process_image_upload("legacy.bmp", &source, "image/bmp").unwrap();

        assert_eq!(processed.filename, "legacy.png");
        assert_eq!(processed.mime_type, "image/png");
        let decoded =
            image::load_from_memory_with_format(&processed.bytes, image::ImageFormat::Png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (16, 8));
    }

    #[test]
    fn static_webp_upload_is_converted_to_png() {
        let source = encode_test_image(12, 6, image::ImageFormat::WebP);

        let processed = process_image_upload("still.webp", &source, "image/webp").unwrap();

        assert_eq!(processed.filename, "still.png");
        assert_eq!(processed.mime_type, "image/png");
        let decoded =
            image::load_from_memory_with_format(&processed.bytes, image::ImageFormat::Png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (12, 6));
    }

    #[test]
    fn animated_webp_upload_is_converted_to_animated_gif() {
        use image::AnimationDecoder as _;

        // 2x2、两帧、无限循环的 WebP：红帧 80ms，蓝帧 120ms。
        const ANIMATED_WEBP: &[u8] = &[
            0x52, 0x49, 0x46, 0x46, 0x84, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50,
            0x38, 0x58, 0x0a, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01,
            0x00, 0x00, 0x41, 0x4e, 0x49, 0x4d, 0x06, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
            0x00, 0x00, 0x41, 0x4e, 0x4d, 0x46, 0x28, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x50, 0x00, 0x00, 0x02, 0x56, 0x50,
            0x38, 0x4c, 0x0f, 0x00, 0x00, 0x00, 0x2f, 0x01, 0x40, 0x00, 0x00, 0x07, 0x10, 0xe5,
            0x8f, 0xfe, 0x07, 0x22, 0xa2, 0xff, 0x01, 0x00, 0x41, 0x4e, 0x4d, 0x46, 0x28, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00,
            0x78, 0x00, 0x00, 0x00, 0x56, 0x50, 0x38, 0x4c, 0x0f, 0x00, 0x00, 0x00, 0x2f, 0x01,
            0x40, 0x00, 0x00, 0x07, 0x10, 0xd1, 0xfe, 0xfe, 0x07, 0x22, 0xa2, 0xff, 0x01, 0x00,
        ];

        let processed =
            process_image_upload("animation.webp", ANIMATED_WEBP, "image/webp").unwrap();

        assert_eq!(processed.filename, "animation.gif");
        assert_eq!(processed.mime_type, "image/gif");
        let decoder =
            image::codecs::gif::GifDecoder::new(std::io::Cursor::new(&processed.bytes)).unwrap();
        assert_eq!(decoder.into_frames().count(), 2);
    }

    // 验证损坏的 JPEG 数据在解码阶段会被拒绝并返回错误，而不是 panic。
    #[test]
    fn corrupt_jpeg_upload_is_rejected() {
        let result = process_image_upload("broken.jpg", b"not a jpeg", "image/jpeg");

        assert!(result.is_err());
    }
}
