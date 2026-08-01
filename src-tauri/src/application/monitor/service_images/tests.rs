use std::{
    fs,
    io::{Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::PathBuf,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use super::*;
use crate::domain::monitor::{DiscoveredMonitorDevice, DiscoverySource};

fn loaded_service(test_name: &str) -> (MonitorService, PathBuf) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ai-monitor-{test_name}-{}-{unique}",
        std::process::id()
    ));
    let config_home = root.join("home");
    fs::create_dir_all(&config_home).unwrap();
    let service = MonitorService::load(&root.join("app-data"), &config_home).unwrap();
    (service, root)
}

fn device(id: &str, base_url: &str) -> DiscoveredMonitorDevice {
    DiscoveredMonitorDevice {
        id: id.to_owned(),
        name: format!("Monitor {id}"),
        api_version: "1".to_owned(),
        base_url: base_url.to_owned(),
        path: "/api/device".to_owned(),
        discovery_source: DiscoverySource::Mdns,
    }
}

fn read_request_head(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let length = stream.read(&mut buffer).unwrap();
        assert_ne!(length, 0, "request ended before its headers were complete");
        request.extend_from_slice(&buffer[..length]);
    }
    String::from_utf8(request).unwrap()
}

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

    assert_eq!(
        validate_image_uploads(&images),
        Err("invalid.tiff 不是支持的 BMP、JPEG、GIF、PNG 或 WebP 图片".to_owned())
    );
}

#[test]
fn batch_image_validation_accepts_all_supported_upload_types() {
    let images = [
        ("legacy.bmp", "image/bmp"),
        ("legacy-alias.bmp", "image/x-ms-bmp"),
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

#[test]
fn data_urls_use_rfc4648_standard_base64_with_padding() {
    let vectors = [
        (b"".as_slice(), ""),
        (b"f".as_slice(), "Zg=="),
        (b"fo".as_slice(), "Zm8="),
        (b"foo".as_slice(), "Zm9v"),
        (b"foob".as_slice(), "Zm9vYg=="),
        (b"fooba".as_slice(), "Zm9vYmE="),
        (b"foobar".as_slice(), "Zm9vYmFy"),
        ([0xFB, 0xFF].as_slice(), "+/8="),
    ];

    for (bytes, encoded) in vectors {
        assert_eq!(
            image_data_url(ImageFormat::Png, bytes),
            format!("data:image/png;base64,{encoded}")
        );
    }
}

#[test]
fn remote_image_content_type_is_strict_case_insensitive_and_header_first() {
    assert_eq!(
        image_format_from_content_type(" IMAGE/PNG; CHARSET=utf-8 "),
        Some(ImageFormat::Png)
    );
    assert_eq!(
        remote_image_format(Some("image/gif; version=1"), "image/jpeg"),
        Some(ImageFormat::Gif),
        "a supported response header must win over metadata"
    );
    assert_eq!(
        remote_image_format(Some("image/png; broken"), "IMAGE/JPEG"),
        Some(ImageFormat::Jpeg),
        "an invalid header must fall back to valid metadata"
    );
    assert_eq!(
        remote_image_format(Some("image/webp"), "image/gif"),
        Some(ImageFormat::Gif),
        "an unsupported header must fall back to valid metadata"
    );
    assert_eq!(
        remote_image_format(Some("not a content type"), "image/tiff"),
        None
    );
}

#[test]
fn remote_image_serializes_an_explicit_format_contract() {
    let image = RemoteImage {
        filename: "photo.jpg".to_owned(),
        format: ImageFormat::Jpeg,
        image: "data:image/jpeg;base64,AA==".to_owned(),
    };

    assert_eq!(
        serde_json::to_value(image).unwrap(),
        serde_json::json!({
            "filename": "photo.jpg",
            "format": "jpeg",
            "image": "data:image/jpeg;base64,AA==",
        })
    );
}

#[test]
fn gallery_aggregates_explicit_formats_and_serializes_zero_counts() {
    let gallery = RemoteImageGallery::from_images(vec![
        RemoteImage {
            filename: "first.jpg".to_owned(),
            format: ImageFormat::Jpeg,
            image: String::new(),
        },
        RemoteImage {
            filename: "second.jpg".to_owned(),
            format: ImageFormat::Jpeg,
            image: String::new(),
        },
        RemoteImage {
            filename: "icon.png".to_owned(),
            format: ImageFormat::Png,
            image: String::new(),
        },
    ]);

    assert_eq!(
        gallery.counts,
        RemoteImageCounts {
            jpeg: 2,
            png: 1,
            gif: 0,
        }
    );
    let serialized = serde_json::to_value(gallery).unwrap();
    assert_eq!(
        serialized["counts"],
        serde_json::json!({ "jpeg": 2, "png": 1, "gif": 0 })
    );
    assert_eq!(serialized["images"][0]["format"], "jpeg");
}

#[test]
fn batch_image_validation_rejects_an_empty_selection() {
    assert_eq!(
        validate_image_uploads(&[]),
        Err("请选择要上传的图片".to_owned())
    );
}

#[test]
fn remote_image_url_encodes_one_filename_path_segment() {
    let url = remote_image_url("http://192.168.50.20:8080", "状态 图片 #1?.gif").unwrap();

    assert_eq!(
        url.as_str(),
        "http://192.168.50.20:8080/api/images/%E7%8A%B6%E6%80%81%20%E5%9B%BE%E7%89%87%20%231%3F.gif"
    );
    for invalid in [
        "",
        ".",
        "..",
        "../secret",
        "folder/name.png",
        r"folder\name.png",
    ] {
        assert!(remote_image_url("http://192.168.50.20:8080", invalid).is_err());
    }
}

#[test]
fn all_image_operations_reject_an_offline_current_device() {
    let (service, root) = loaded_service("offline-image-operations");
    service
        .select_device(&device("screen-1", "http://127.0.0.1:1"))
        .unwrap();

    let (list, upload, delete) = tauri::async_runtime::block_on(async {
        (
            service.images("screen-1").await.map(|_| ()),
            service
                .upload_images(
                    "screen-1",
                    vec![ImageUpload {
                        filename: "image.png".to_owned(),
                        mime_type: "image/png".to_owned(),
                        bytes: vec![1],
                    }],
                )
                .await
                .map(|_| ()),
            service.delete_image("screen-1", "image.png").await,
        )
    });

    let expected = Err("当前 AIMonitor 设备不在线".to_owned());
    assert_eq!(list, expected);
    assert_eq!(upload, expected);
    assert_eq!(delete, expected);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn all_image_operations_reject_a_stale_device_token_before_http() {
    let (service, root) = loaded_service("stale-image-device-token");
    let current = device("screen-1", "http://127.0.0.1:1");
    service.select_device(&current).unwrap();
    *service.online_devices.write().unwrap() = vec![current];

    let (list, upload, delete) = tauri::async_runtime::block_on(async {
        (
            service.images("screen-old").await.map(|_| ()),
            service
                .upload_images(
                    "screen-old",
                    vec![ImageUpload {
                        filename: "image.png".to_owned(),
                        mime_type: "image/png".to_owned(),
                        bytes: vec![1],
                    }],
                )
                .await
                .map(|_| ()),
            service.delete_image("screen-old", "image.png").await,
        )
    });

    let expected = Err("当前设备已切换，请重新执行操作".to_owned());
    assert_eq!(list, expected);
    assert_eq!(upload, expected);
    assert_eq!(delete, expected);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn delete_uses_latest_online_address_and_an_encoded_filename_path() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request_head(&mut stream);
        assert!(request.starts_with(
            "DELETE /api/images/%E7%8A%B6%E6%80%81%20%E5%9B%BE%E7%89%87%20%231%3F.gif HTTP/1.1\r\n"
        ));
        stream
            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
            .unwrap();
    });
    let (service, root) = loaded_service("delete-image-url");
    // 持久化地址故意指向无效端口，只有在线快照的最新地址可达。
    service
        .select_device(&device("screen-1", "http://127.0.0.1:1"))
        .unwrap();
    *service.online_devices.write().unwrap() =
        vec![device("screen-1", &format!("http://{address}"))];

    tauri::async_runtime::block_on(service.delete_image("screen-1", "状态 图片 #1?.gif")).unwrap();

    server.join().unwrap();
    fs::remove_dir_all(root).unwrap();
}
