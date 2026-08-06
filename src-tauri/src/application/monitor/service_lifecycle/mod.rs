// MonitorService 生命周期与配置管理：加载/持久化本地数据、保存设置、
// 管理 AI Profile 与 Hook 配置文件的读写。
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

use uuid::Uuid;

use super::{
    DeviceSnapshotState, HOOK_BIND_ADDRESS, HOOK_LISTENER_PORT, HookRelayStatus, MonitorService,
    STORE_FILENAME, build_monitor_client,
    config_io::{read_optional_config, write_atomic_file, write_config},
    hook_config::{detect_hook_config_directories, detect_system_username},
    wsl::WslDirectory,
};
use crate::domain::AppError;
use crate::domain::monitor::{
    AiProfile, AiProfileDraft, AiProfileDraftSet, AiTool, HookConfigLocation,
    HookConfigWriteResult, MonitorSettings, SavedMonitorData, generate_hook_auxiliary_configs,
    generate_hook_config, generate_wsl_hook_config, hook_config_filename, hook_config_write_result,
    hook_supports_wsl, merge_hook_config, normalize_enabled_ai_tools,
    validate_discovery_interval_minutes, validate_profile, validate_saved_monitor_data,
    validate_username,
};

#[cfg(test)]
mod tests;

impl MonitorService {
    // 加载/初始化服务：确保配置目录存在，读取（或创建默认的）本地持久化数据，
    // 校验数据合法性后构造出内存态的各共享状态。
    pub fn load(app_data_dir: &Path, config_home: &Path) -> Result<Self, AppError> {
        // 应用数据目录不存在则递归创建。
        fs::create_dir_all(app_data_dir).map_err(|error| {
            AppError::new("error.lifecycle.createConfigDirFailed")
                .param("detail", error.to_string())
        })?;
        let data_path = app_data_dir.join(STORE_FILENAME);
        let mut data = if data_path.exists() {
            // 存储文件已存在：读取内容并反序列化为 SavedMonitorData。
            let contents = fs::read_to_string(&data_path).map_err(|error| {
                AppError::new("error.lifecycle.readConfigFailed").param("detail", error.to_string())
            })?;
            serde_json::from_str(&contents).map_err(|error| {
                AppError::new("error.lifecycle.configFormatInvalid")
                    .param("detail", error.to_string())
            })?
        } else {
            // 首次启动：使用默认空数据。
            SavedMonitorData::default()
        };
        let client_id_generated = ensure_client_id(&mut data);
        if data.settings.username.trim().is_empty()
            && let Some(username) = detect_system_username(config_home)
        {
            data.settings.username = username;
        }
        // 无论是读取到的还是默认数据，都要过一遍领域层校验，防止带着非法数据启动。
        validate_saved_monitor_data(&data)?;
        if client_id_generated {
            // 只在首次生成时写回，避免每次启动都重新生成/持久化同一个稳定身份。
            persist_to(&data_path, &data)?;
        }
        let (hook_device_snapshot_revision, _) = tokio::sync::watch::channel(0);
        Ok(Self {
            client: build_monitor_client()?,
            data_path,
            default_hook_config_directories: detect_hook_config_directories(config_home),
            data: Arc::new(RwLock::new(data)),
            online_devices: Arc::new(RwLock::new(Vec::new())),
            discovery_missed_scans: Arc::new(Mutex::new(HashMap::new())),
            device_snapshot_state: Arc::new(Mutex::new(DeviceSnapshotState::default())),
            hook_config_write_lock: Arc::new(Mutex::new(())),
            hook_device_snapshot_revision,
            // 中继状态初始值：尚未开始监听，绑定地址/端口先填好，其余字段用 Default。
            relay_status: Arc::new(RwLock::new(HookRelayStatus {
                bind_address: HOOK_BIND_ADDRESS.to_owned(),
                port: HOOK_LISTENER_PORT,
                ..HookRelayStatus::default()
            })),
        })
    }

    // 读取当前持久化的设置数据（克隆一份返回，避免长期持有锁）。
    pub fn settings(&self) -> Result<MonitorSettings, AppError> {
        self.data
            .read()
            .map(|data| data.settings.clone())
            .map_err(|_| AppError::new("error.internal.lockPoisoned"))
    }

    // 保存用户名：先做领域层校验，再持久化写入设置。
    pub fn save_username(&self, username: &str) -> Result<MonitorSettings, AppError> {
        let username = validate_username(username)?;
        let mut data = self
            .data
            .write()
            .map_err(|_| AppError::new("error.internal.lockPoisoned"))?;
        let mut next_data = data.clone();
        next_data.settings.username = username;
        self.persist(&next_data)?;
        *data = next_data;
        Ok(data.settings.clone())
    }

    // 保存用户在设置页配置的自动发现检查间隔（分钟），校验后持久化并返回最新设置。
    pub fn save_discovery_interval(&self, minutes: u64) -> Result<MonitorSettings, AppError> {
        let minutes = validate_discovery_interval_minutes(minutes)?;
        let mut data = self
            .data
            .write()
            .map_err(|_| AppError::new("error.internal.lockPoisoned"))?;
        let mut next_data = data.clone();
        next_data.settings.discovery_interval_minutes = minutes;
        self.persist(&next_data)?;
        *data = next_data;
        Ok(data.settings.clone())
    }

    /// 保存设置页勾选的 AI 客户端，按固定顺序去重后供两个管理页面共同使用。
    pub fn save_enabled_ai_tools(&self, tools: &[AiTool]) -> Result<MonitorSettings, AppError> {
        let tools = normalize_enabled_ai_tools(tools);
        let settings = {
            let mut data = self
                .data
                .write()
                .map_err(|_| AppError::new("error.internal.lockPoisoned"))?;
            let mut next_data = data.clone();
            next_data.settings.enabled_ai_tools = tools;
            self.persist(&next_data)?;
            *data = next_data;
            data.settings.clone()
        };

        // 必须先释放 data 写锁：自动写入会重新读取设置与 Hooks 路径。
        // 写入采用 best-effort 语义，失败不回滚已经持久化成功的 AI 选择。
        self.start_auto_write_enabled_hook_configs();
        Ok(settings)
    }

    #[cfg(test)]
    pub fn profiles(&self) -> Result<Vec<AiProfile>, AppError> {
        self.data
            .read()
            .map(|data| {
                data.profiles
                    .iter()
                    .filter(|profile| profile.device_id == data.settings.device_id)
                    .cloned()
                    .collect()
            })
            .map_err(|_| AppError::new("error.internal.lockPoisoned"))
    }

    pub fn profile_drafts(&self) -> Result<AiProfileDraftSet, AppError> {
        let data = self
            .data
            .read()
            .map_err(|_| AppError::new("error.internal.lockPoisoned"))?;
        let expected_device_id = data.settings.device_id.clone();
        let drafts = AiTool::ALL
            .into_iter()
            .map(|tool| {
                data.profiles
                    .iter()
                    .find(|profile| profile.device_id == expected_device_id && profile.tool == tool)
                    .map_or_else(|| AiProfileDraft::default_for(tool), AiProfileDraft::from)
            })
            .collect();
        Ok(AiProfileDraftSet {
            expected_device_id,
            drafts,
        })
    }

    pub fn hook_config_locations(&self) -> Result<Vec<HookConfigLocation>, AppError> {
        let data = self
            .data
            .read()
            .map_err(|_| AppError::new("error.internal.lockPoisoned"))?;
        Ok(AiTool::ALL
            .into_iter()
            .map(|tool| self.hook_config_location(&data, tool))
            .collect())
    }

    pub fn save_hook_config_directory(
        &self,
        tool: AiTool,
        directory: &str,
    ) -> Result<HookConfigLocation, AppError> {
        let directory = directory.trim();
        if !directory.is_empty() {
            let path = Path::new(directory);
            if !path.is_absolute() {
                return Err(AppError::new("error.hooks.directoryNotAbsolute"));
            }
            if path.exists() && !path.is_dir() {
                return Err(AppError::new("error.hooks.directoryNotAFolder")
                    .param("path", path.display().to_string()));
            }
        }

        let mut data = self
            .data
            .write()
            .map_err(|_| AppError::new("error.internal.lockPoisoned"))?;
        let mut next_data = data.clone();
        next_data
            .hook_config_directories
            .set(tool, directory.to_owned());
        self.persist(&next_data)?;
        let location = self.hook_config_location(&next_data, tool);
        *data = next_data;
        Ok(location)
    }

    #[cfg(test)]
    pub fn save_profile(&self, profile: AiProfile) -> Result<AiProfile, AppError> {
        let mut data = self
            .data
            .write()
            .map_err(|_| AppError::new("error.internal.lockPoisoned"))?;
        self.save_profile_for_current_device(&mut data, profile)
    }

    pub fn save_profile_draft(
        &self,
        expected_device_id: &str,
        draft: AiProfileDraft,
    ) -> Result<AiProfileDraft, AppError> {
        let mut data = self
            .data
            .write()
            .map_err(|_| AppError::new("error.internal.lockPoisoned"))?;
        if data.settings.device_id != expected_device_id {
            return Err(AppError::new("error.profile.deviceChangedReload"));
        }
        let profile = draft.bind_to_device(expected_device_id);
        self.save_profile_for_current_device(&mut data, profile)
            .map(AiProfileDraft::from)
    }

    fn save_profile_for_current_device(
        &self,
        data: &mut SavedMonitorData,
        mut profile: AiProfile,
    ) -> Result<AiProfile, AppError> {
        if data.settings.device_id.is_empty() {
            return Err(AppError::new("error.connection.deviceNotSelected"));
        }
        profile.device_id.clone_from(&data.settings.device_id);
        let profile = validate_profile(profile)?;
        let mut next_data = data.clone();
        next_data.profiles.retain(|existing| {
            existing.device_id != profile.device_id || existing.tool != profile.tool
        });
        next_data.profiles.push(profile.clone());
        next_data.profiles.sort_by(|left, right| {
            left.device_id
                .cmp(&right.device_id)
                .then(left.slot.cmp(&right.slot))
        });
        self.persist(&next_data)?;
        *data = next_data;
        Ok(profile)
    }

    // 将某个 AI 工具的 Hook 配置写入其配置文件：生成新内容、与已有文件合并，
    // 仅在内容真正变化时才落盘写入，并告知调用方该工具是否需要人工复核/重启。
    pub fn write_hook_config(&self, tool: AiTool) -> Result<HookConfigWriteResult, AppError> {
        // 用互斥锁串行化配置文件写入，避免并发写入互相覆盖或撕裂文件内容。
        let _write_guard = self
            .hook_config_write_lock
            .lock()
            .map_err(|_| AppError::new("error.internal.lockPoisoned"))?;
        let data = self
            .data
            .read()
            .map_err(|_| AppError::new("error.internal.lockPoisoned"))?;
        // Hook 只连接固定的本机中继，不依赖设备 Profile，可在展示配置之前写入。
        // 命令型 Hook 复用当前 AIMonitor 可执行文件的轻量 relay 子命令，因此把
        // 已安装程序的绝对路径固化进配置；移动安装位置后重新执行一次写入即可。
        let relay_executable = std::env::current_exe().map_err(|error| {
            AppError::new("error.hooks.locateRelayExecutableFailed")
                .param("detail", error.to_string())
        })?;
        let location = self.hook_config_location(&data, tool);
        let wsl_directory = WslDirectory::parse(&location.directory);
        if wsl_directory.is_some() && !hook_supports_wsl(tool) {
            return Err(AppError::new("error.hooks.wslUnsupportedByWindowsHost")
                .param("tool", crate::domain::monitor::ai_tool_name(tool)));
        }
        let generated = if let Some(wsl_directory) = &wsl_directory {
            let wsl_executable = wsl_directory.translate_windows_executable(&relay_executable)?;
            generate_wsl_hook_config(tool, &relay_executable, &wsl_executable)?
        } else {
            generate_hook_config(tool, &relay_executable)?
        };
        let config_path = PathBuf::from(&location.config_path);

        if let Some(wsl_directory) = wsl_directory {
            let mut generated_files =
                vec![(wsl_directory.join(hook_config_filename(tool)), generated)];
            generated_files.extend(
                generate_hook_auxiliary_configs(tool)
                    .into_iter()
                    .map(|preview| (wsl_directory.join(&preview.filename), preview)),
            );
            let mut writes = Vec::with_capacity(generated_files.len());
            for (path, generated) in generated_files {
                let existing = path.read_optional()?;
                let merged = merge_hook_config(existing.as_deref(), &generated, tool)?;
                let changed = existing.as_deref() != Some(merged.content.as_str());
                writes.push((path, merged, changed));
            }
            let config_changed = writes.iter().any(|(_, _, changed)| *changed);
            for (path, merged, changed) in &writes {
                if *changed {
                    path.write_atomic(&merged.content).map_err(|error| {
                        AppError::new("error.hooks.writeFailed")
                            .param("path", path.display().to_string())
                            .param("detail", error.to_string())
                    })?;
                }
            }
            return Ok(hook_config_write_result(
                tool,
                config_path.to_string_lossy().into_owned(),
                config_changed,
            ));
        }

        let mut generated_files = vec![(config_path.clone(), generated)];
        generated_files.extend(
            generate_hook_auxiliary_configs(tool)
                .into_iter()
                .map(|preview| {
                    (
                        Path::new(&location.directory).join(&preview.filename),
                        preview,
                    )
                }),
        );

        // 所有目标先读取并完成冲突/格式校验，再统一写入；任一文件不安全时不留下半套配置。
        let mut writes = Vec::with_capacity(generated_files.len());
        for (path, generated) in generated_files {
            let existing = read_optional_config(&path)?;
            let merged = merge_hook_config(existing.as_deref(), &generated, tool)?;
            let changed = existing.as_deref() != Some(merged.content.as_str());
            writes.push((path, merged, changed));
        }
        let config_changed = writes.iter().any(|(_, _, changed)| *changed);
        for (path, merged, changed) in &writes {
            if *changed {
                write_config(path, &merged.content)?;
            }
        }
        Ok(hook_config_write_result(
            tool,
            config_path.to_string_lossy().into_owned(),
            config_changed,
        ))
    }

    // 计算某个工具的 Hook 配置目录与文件路径：优先用户自定义目录，否则使用探测到的默认目录。
    fn hook_config_location(&self, data: &SavedMonitorData, tool: AiTool) -> HookConfigLocation {
        let custom_directory = data.hook_config_directories.get(tool);
        let directory = if custom_directory.is_empty() {
            self.default_hook_config_directories.get(tool)
        } else {
            custom_directory
        };
        let config_path = Path::new(directory).join(hook_config_filename(tool));
        HookConfigLocation {
            tool,
            directory: directory.to_owned(),
            config_path: config_path.to_string_lossy().into_owned(),
            is_custom: !custom_directory.is_empty(),
        }
    }

    // 将内存中的数据序列化为格式化 JSON 并原子写入磁盘存储文件。
    pub(super) fn persist(&self, data: &SavedMonitorData) -> Result<(), AppError> {
        persist_to(&self.data_path, data)
    }
}

// 若尚未持久化过控制端身份，则生成一个新的稳定 `clientId`。
// 返回是否生成了新值，供调用方决定是否需要把这次生成结果写回磁盘。
fn ensure_client_id(data: &mut SavedMonitorData) -> bool {
    if data.client_id.trim().is_empty() {
        data.client_id = Uuid::new_v4().to_string();
        true
    } else {
        false
    }
}

// 将数据序列化为格式化 JSON 并原子写入指定路径；`load()` 在 `Self` 构造前
// 需要这份逻辑，`persist` 方法在构造后复用同一实现。
fn persist_to(path: &Path, data: &SavedMonitorData) -> Result<(), AppError> {
    let serialized = serde_json::to_string_pretty(data).map_err(|error| {
        AppError::new("error.lifecycle.serializeConfigFailed").param("detail", error.to_string())
    })?;
    write_atomic_file(path, &serialized)
}
