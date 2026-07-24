use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::RwLock,
    time::{Duration, Instant},
};

use mdns_sd::{ServiceDaemon, ServiceEvent};
use reqwest::{Client, StatusCode, multipart};
use serde::{Deserialize, Serialize};

use crate::domain::monitor::{
    AiProfile, AiTool, DiscoveredMonitorDevice, HookConfigPreview, HookConfigWriteResult,
    LocalHookConfig, MonitorSettings, SavedMonitorData, generate_hook_config,
    inspect_local_hook_config, merge_hook_config, migrate_legacy_profile, normalize_base_url,
    validate_profile, validate_settings,
};

const STORE_FILENAME: &str = "monitor-data.json";
const AIMONITOR_SERVICE_TYPE: &str = "_aimonitor._tcp.local.";
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(2);

pub struct MonitorService {
    client: Client,
    data_path: PathBuf,
    config_home: PathBuf,
    data: RwLock<SavedMonitorData>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub reachable: bool,
    pub base_url: String,
    pub message: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteImage {
    pub filename: String,
    pub mime_type: String,
    pub image: String,
}

#[derive(Deserialize)]
struct ImageListResponse {
    images: Vec<RemoteImage>,
}

#[derive(Deserialize)]
struct UploadResponse {
    filename: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageUpload {
    filename: String,
    mime_type: String,
    bytes: Vec<u8>,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
}

impl MonitorService {
    pub fn load(app_data_dir: &Path, config_home: &Path) -> Result<Self, String> {
        fs::create_dir_all(app_data_dir).map_err(|error| format!("无法创建配置目录：{error}"))?;
        let data_path = app_data_dir.join(STORE_FILENAME);
        let mut data = if data_path.exists() {
            let contents =
                fs::read_to_string(&data_path).map_err(|error| format!("无法读取配置：{error}"))?;
            serde_json::from_str(&contents).map_err(|error| format!("配置文件格式错误：{error}"))?
        } else {
            SavedMonitorData::default()
        };
        for profile in &mut data.profiles {
            migrate_legacy_profile(profile);
        }

        Ok(Self {
            client: Client::new(),
            data_path,
            config_home: config_home.to_owned(),
            data: RwLock::new(data),
        })
    }

    pub fn settings(&self) -> Result<MonitorSettings, String> {
        self.data
            .read()
            .map(|data| data.settings.clone())
            .map_err(|_| "配置读取锁已损坏".to_owned())
    }

    pub fn save_settings(
        &self,
        device: &DiscoveredMonitorDevice,
        username: &str,
    ) -> Result<MonitorSettings, String> {
        let settings = validate_settings(device, username)?;
        let mut data = self
            .data
            .write()
            .map_err(|_| "配置写入锁已损坏".to_owned())?;
        data.settings = settings.clone();
        self.persist(&data)?;
        Ok(settings)
    }

    pub fn discover_devices() -> Result<Vec<DiscoveredMonitorDevice>, String> {
        let daemon = ServiceDaemon::new().map_err(|error| format!("无法启动设备发现：{error}"))?;
        let receiver = daemon
            .browse(AIMONITOR_SERVICE_TYPE)
            .map_err(|error| format!("无法扫描 AIMonitor 设备：{error}"))?;
        let deadline = Instant::now() + DISCOVERY_TIMEOUT;
        let mut devices = HashMap::new();

        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            match receiver.recv_timeout(remaining) {
                Ok(ServiceEvent::ServiceResolved(service)) => {
                    let properties = service.get_properties();
                    let id = properties
                        .get_property_val_str("id")
                        .unwrap_or(service.get_fullname())
                        .to_owned();
                    let name = properties
                        .get_property_val_str("name")
                        .unwrap_or(service.get_fullname())
                        .to_owned();
                    let api_version = properties
                        .get_property_val_str("apiVersion")
                        .unwrap_or("1")
                        .to_owned();
                    let path = properties
                        .get_property_val_str("path")
                        .unwrap_or("/api/device")
                        .to_owned();
                    let mut addresses: Vec<_> = service.get_addresses_v4().into_iter().collect();
                    addresses.sort_unstable();
                    let host = addresses.first().map_or_else(
                        || service.get_hostname().trim_end_matches('.').to_owned(),
                        ToString::to_string,
                    );
                    let base_url = format!("http://{host}:{}", service.get_port());
                    devices.insert(
                        id.clone(),
                        DiscoveredMonitorDevice {
                            id,
                            name,
                            api_version,
                            base_url,
                            path,
                        },
                    );
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }

        let _ = daemon.stop_browse(AIMONITOR_SERVICE_TYPE);
        let _ = daemon.shutdown();
        let mut devices: Vec<_> = devices.into_values().collect();
        devices.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(devices)
    }

    pub async fn check_connection(
        &self,
        base_url: Option<&str>,
    ) -> Result<ConnectionStatus, String> {
        let base_url = match base_url {
            Some(value) => normalize_base_url(value)?,
            None => self.settings()?.base_url,
        };
        let result = self
            .client
            .get(format!("{base_url}/health"))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;

        Ok(match result {
            Ok(response) if response.status().is_success() => ConnectionStatus {
                reachable: true,
                base_url,
                message: "设备连接正常".to_owned(),
            },
            Ok(response) => ConnectionStatus {
                reachable: false,
                base_url,
                message: format!("设备返回 HTTP {}", response.status().as_u16()),
            },
            Err(error) => ConnectionStatus {
                reachable: false,
                base_url,
                message: format!("无法连接设备：{error}"),
            },
        })
    }

    pub async fn images(&self) -> Result<Vec<RemoteImage>, String> {
        let base_url = self.settings()?.base_url;
        let response = self
            .client
            .get(format!("{base_url}/api/images"))
            .send()
            .await
            .map_err(|error| format!("无法读取远端图片：{error}"))?;
        let response = ensure_success(response).await?;
        response
            .json::<ImageListResponse>()
            .await
            .map(|body| body.images)
            .map_err(|error| format!("图片列表响应格式错误：{error}"))
    }

    pub async fn upload_images(&self, images: Vec<ImageUpload>) -> Result<Vec<String>, String> {
        validate_image_uploads(&images)?;
        let base_url = self.settings()?.base_url;
        let mut uploaded = Vec::with_capacity(images.len());

        for image in images {
            let filename = image.filename.clone();
            let file_part = multipart::Part::bytes(image.bytes)
                .file_name(filename.clone())
                .mime_str(&image.mime_type)
                .map_err(|error| format!("{filename} 的图片类型无效：{error}"))?;
            let response = self
                .client
                .post(format!("{base_url}/api/images"))
                .multipart(multipart::Form::new().part("file", file_part))
                .send()
                .await
                .map_err(|error| format!("{filename} 上传失败：{error}"))?;
            let response = ensure_success(response).await?;
            let uploaded_filename = response
                .json::<UploadResponse>()
                .await
                .map(|body| body.filename)
                .map_err(|error| format!("{filename} 的上传响应格式错误：{error}"))?;
            uploaded.push(uploaded_filename);
        }

        Ok(uploaded)
    }

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

    pub fn profiles(&self) -> Result<Vec<AiProfile>, String> {
        self.data
            .read()
            .map(|data| data.profiles.clone())
            .map_err(|_| "AI 配置读取锁已损坏".to_owned())
    }

    pub fn write_profile(&self, profile: AiProfile) -> Result<HookConfigWriteResult, String> {
        let profile = validate_profile(profile)?;
        let mut data = self
            .data
            .write()
            .map_err(|_| "AI 配置写入锁已损坏".to_owned())?;
        let settings = data.settings.clone();
        let generated = generate_hook_config(&settings, profile.clone())?;
        let config_path = self.config_home.join(&generated.filename);
        let existing = read_optional_config(&config_path)?;
        let merged = merge_hook_config(existing.as_deref(), &generated, profile.tool, &settings)?;
        let config_changed = existing.as_deref() != Some(merged.content.as_str());

        let mut next_data = data.clone();
        next_data
            .profiles
            .retain(|existing| existing.tool != profile.tool);
        next_data.profiles.push(profile.clone());
        next_data.profiles.sort_by_key(|item| item.slot);

        if config_changed {
            write_config(&config_path, &merged.content)?;
        }
        if let Err(error) = self.persist(&next_data) {
            if config_changed
                && let Err(rollback_error) =
                    restore_optional_config(&config_path, existing.as_deref())
            {
                return Err(format!(
                    "{error}；Hooks 配置回滚失败，当前文件可能已发生变化：{rollback_error}"
                ));
            }
            return Err(error);
        }
        *data = next_data;

        Ok(HookConfigWriteResult {
            requires_review: profile.tool == AiTool::Codex && config_changed,
            restart_required: profile.tool == AiTool::Codex && config_changed,
            profile,
            filename: merged.filename,
            config_changed,
        })
    }

    pub fn hook_config_preview(&self, profile: AiProfile) -> Result<HookConfigPreview, String> {
        let settings = self.settings()?;
        let tool = profile.tool;
        let generated = generate_hook_config(&settings, profile)?;
        let config_path = self.config_home.join(&generated.filename);
        let existing = read_optional_config(&config_path)?;
        merge_hook_config(existing.as_deref(), &generated, tool, &settings)
    }

    pub fn local_hook_configs(&self) -> Result<Vec<LocalHookConfig>, String> {
        [
            (AiTool::Codex, ".codex/hooks.json"),
            (AiTool::ClaudeCode, ".claude/settings.json"),
            (AiTool::Cursor, ".cursor/hooks.json"),
        ]
        .into_iter()
        .map(|(tool, filename)| {
            let content = read_optional_config(&self.config_home.join(filename))?;
            Ok(inspect_local_hook_config(
                tool,
                filename.to_owned(),
                content,
            ))
        })
        .collect()
    }

    fn persist(&self, data: &SavedMonitorData) -> Result<(), String> {
        let serialized = serde_json::to_string_pretty(data)
            .map_err(|error| format!("无法序列化配置：{error}"))?;
        write_atomic_file(&self.data_path, &serialized, "应用配置")
    }
}

fn validate_image_uploads(images: &[ImageUpload]) -> Result<(), String> {
    if images.is_empty() {
        return Err("请选择要上传的图片".to_owned());
    }

    let allowed = ["image/jpeg", "image/png", "image/gif"];
    for image in images {
        if image.filename.trim().is_empty() || image.bytes.is_empty() {
            return Err("所选图片中包含空文件".to_owned());
        }
        if image.bytes.len() > 8 * 1024 * 1024 {
            return Err(format!("{} 不能超过 8 MiB", image.filename));
        }
        if !allowed.contains(&image.mime_type.as_str()) {
            return Err(format!(
                "{} 不是支持的 JPEG、PNG 或 GIF 图片",
                image.filename
            ));
        }
    }

    Ok(())
}

fn read_optional_config(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("无法读取 {}：{error}", path.display())),
    }
}

fn write_config(path: &Path, content: &str) -> Result<(), String> {
    write_atomic_file(path, content, "Hooks 配置")
}

fn write_atomic_file(path: &Path, content: &str, label: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("无法确定 {} 的配置目录", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("无法创建配置目录 {}：{error}", parent.display()))?;

    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("配置文件路径无效：{}", path.display()))?;
    let temporary_path = parent.join(format!(".{filename}.aimonitor.tmp"));
    fs::write(&temporary_path, content)
        .map_err(|error| format!("无法写入临时配置 {}：{error}", temporary_path.display()))?;

    #[cfg(not(windows))]
    let replace_result = fs::rename(&temporary_path, path);
    #[cfg(windows)]
    let replace_result = fs::write(path, content).and_then(|()| fs::remove_file(&temporary_path));

    if let Err(error) = replace_result {
        let _ = fs::remove_file(&temporary_path);
        return Err(format!("无法写入{label} {}：{error}", path.display()));
    }
    Ok(())
}

fn restore_optional_config(path: &Path, content: Option<&str>) -> Result<(), String> {
    if let Some(content) = content {
        return write_config(path, content);
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("无法删除新建配置 {}：{error}", path.display())),
    }
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let fallback = format!("设备请求失败（HTTP {}）", status.as_u16());
    if status == StatusCode::NO_CONTENT {
        return Err(fallback);
    }
    let message = response
        .json::<ErrorResponse>()
        .await
        .map_or(fallback, |body| body.error);
    Err(message)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::domain::monitor::{HookBehavior, HookContent};

    use super::*;

    fn test_profile() -> AiProfile {
        AiProfile {
            tool: AiTool::Codex,
            slot: 1,
            hooks: [
                (HookBehavior::Idle, "idle.png"),
                (HookBehavior::Running, "running.gif"),
                (HookBehavior::Asking, "asking.png"),
                (HookBehavior::Error, "error.png"),
            ]
            .into_iter()
            .map(|(behavior, image)| HookContent {
                behavior,
                content: String::new(),
                image: image.to_owned(),
            })
            .collect(),
        }
    }

    #[test]
    fn batch_image_validation_checks_every_file_before_upload() {
        let images = vec![
            ImageUpload {
                filename: "valid.png".to_owned(),
                mime_type: "image/png".to_owned(),
                bytes: vec![1],
            },
            ImageUpload {
                filename: "invalid.webp".to_owned(),
                mime_type: "image/webp".to_owned(),
                bytes: vec![1],
            },
        ];

        assert_eq!(
            validate_image_uploads(&images),
            Err("invalid.webp 不是支持的 JPEG、PNG 或 GIF 图片".to_owned())
        );
    }

    #[test]
    fn batch_image_validation_rejects_an_empty_selection() {
        assert_eq!(
            validate_image_uploads(&[]),
            Err("请选择要上传的图片".to_owned())
        );
    }

    #[test]
    fn profile_write_rolls_back_hook_file_when_data_persistence_fails() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ai-monitor-hook-transaction-{}-{unique}",
            std::process::id()
        ));
        let config_home = root.join("home");
        let invalid_data_path = root.join("data-path-is-a-directory");
        fs::create_dir_all(&invalid_data_path).unwrap();

        let service = MonitorService {
            client: Client::new(),
            data_path: invalid_data_path,
            config_home: config_home.clone(),
            data: RwLock::new(SavedMonitorData {
                settings: MonitorSettings {
                    base_url: "http://127.0.0.1:8080".to_owned(),
                    username: "tester".to_owned(),
                    device_id: "device-1".to_owned(),
                    device_name: "monitor".to_owned(),
                },
                profiles: Vec::new(),
            }),
        };

        let result = service.write_profile(test_profile());

        assert!(result.is_err());
        assert!(!config_home.join(".codex/hooks.json").exists());
        assert!(service.profiles().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
