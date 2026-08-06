// 配置文件的读写工具：读取一个可能不存在的文件、原子写入文件内容。
// 供 `service_lifecycle.rs` 里持久化配置/Hook 文件的逻辑使用。
use std::{fs, io, io::Write, path::Path, path::PathBuf};

use tempfile::{Builder, PersistError};
use tracing::error;

use crate::domain::AppError;

/// 配置文件读写内部错误：用 `thiserror` 集中承载每种失败的路径与来源，
/// 只在模块边界转换成一次 `AppError`，避免每个 `fs`/`tempfile` 调用点各自
/// 手写 `map_err` 拼装 i18n code + params。
#[derive(Debug, thiserror::Error)]
enum ConfigIoError {
    #[error("无法读取配置文件 {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("无法确定 {path} 的上级目录")]
    MissingParentDirectory { path: PathBuf },
    #[error("无法创建目录 {path}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("路径 {path} 不是合法的文件名")]
    InvalidPath { path: PathBuf },
    #[error("无法在 {path} 创建临时文件")]
    CreateTemp {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("无法写入临时文件 {path}")]
    WriteTemp {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("无法同步临时文件 {path}")]
    SyncTemp {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("无法把临时文件替换为 {path}")]
    Persist {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl From<ConfigIoError> for AppError {
    fn from(error: ConfigIoError) -> Self {
        // 统一在转换为面向前端的 AppError 之前记录一条日志：i18n code/params
        // 不包含完整错误链，文件日志保留 thiserror 的 Display（含 `#[source]`）。
        error!(error = %error, "配置文件读写失败");
        let (code, path) = match &error {
            ConfigIoError::Read { path, .. } => ("error.configIo.readFailed", path),
            ConfigIoError::MissingParentDirectory { path } => {
                ("error.configIo.cannotDetermineDirectory", path)
            }
            ConfigIoError::CreateDir { path, .. } => ("error.configIo.createDirFailed", path),
            ConfigIoError::InvalidPath { path } => ("error.configIo.invalidPath", path),
            ConfigIoError::CreateTemp { path, .. } => ("error.configIo.createTempFailed", path),
            ConfigIoError::WriteTemp { path, .. } => ("error.configIo.writeTempFailed", path),
            ConfigIoError::SyncTemp { path, .. } => ("error.configIo.syncTempFailed", path),
            ConfigIoError::Persist { path, .. } => ("error.configIo.persistFailed", path),
        };
        let app_error = AppError::new(code).param("path", path.display().to_string());
        match &error {
            ConfigIoError::MissingParentDirectory { .. } | ConfigIoError::InvalidPath { .. } => {
                app_error
            }
            ConfigIoError::Read { source, .. }
            | ConfigIoError::CreateDir { source, .. }
            | ConfigIoError::CreateTemp { source, .. }
            | ConfigIoError::WriteTemp { source, .. }
            | ConfigIoError::SyncTemp { source, .. }
            | ConfigIoError::Persist { source, .. } => {
                app_error.param("detail", source.to_string())
            }
        }
    }
}

// 读取一个可能尚不存在的配置文件；文件缺失视为正常情况（返回 None），
// 而不是错误——首次写入前，Hook 配置和应用存储文件都可能还不存在。
pub(super) fn read_optional_config(path: &Path) -> Result<Option<String>, AppError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ConfigIoError::Read {
            path: path.to_path_buf(),
            source,
        }
        .into()),
    }
}

// 写入 Hook 配置文件，复用通用的原子写入逻辑。
pub(super) fn write_config(path: &Path, content: &str) -> Result<(), AppError> {
    write_atomic_file(path, content)
}

// 原子写入文件：先写临时文件，再重命名/替换为目标文件，避免写入过程中崩溃导致文件损坏或内容截断。
pub(super) fn write_atomic_file(path: &Path, content: &str) -> Result<(), AppError> {
    write_atomic_file_inner(path, content).map_err(Into::into)
}

fn write_atomic_file_inner(path: &Path, content: &str) -> Result<(), ConfigIoError> {
    let parent = path
        .parent()
        .ok_or_else(|| ConfigIoError::MissingParentDirectory {
            path: path.to_path_buf(),
        })?;
    // 确保目标目录存在。
    fs::create_dir_all(parent).map_err(|source| ConfigIoError::CreateDir {
        path: parent.to_path_buf(),
        source,
    })?;

    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ConfigIoError::InvalidPath {
            path: path.to_path_buf(),
        })?;
    // 随机临时文件必须与目标文件位于同一目录，才能由 persist 在各平台执行
    // 原子替换；NamedTempFile 在任何失败路径上都会自动清理尚未持久化的文件。
    let temporary_prefix = format!(".{filename}.aimonitor-");
    let mut temporary_file = Builder::new()
        .prefix(&temporary_prefix)
        .suffix(".tmp")
        .tempfile_in(parent)
        .map_err(|source| ConfigIoError::CreateTemp {
            path: parent.to_path_buf(),
            source,
        })?;
    let temporary_path = temporary_file.path().to_path_buf();

    temporary_file
        .as_file_mut()
        .write_all(content.as_bytes())
        .map_err(|source| ConfigIoError::WriteTemp {
            path: temporary_path.clone(),
            source,
        })?;
    temporary_file
        .as_file()
        .sync_all()
        .map_err(|source| ConfigIoError::SyncTemp {
            path: temporary_path.clone(),
            source,
        })?;
    temporary_file
        .persist(path)
        .map_err(
            |PersistError { error: source, .. }| ConfigIoError::Persist {
                path: path.to_path_buf(),
                source,
            },
        )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::{Arc, Barrier},
        thread,
    };

    use tempfile::tempdir;

    use super::write_atomic_file;

    #[test]
    fn atomic_write_creates_parent_directories_and_file() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("nested/config.json");

        write_atomic_file(&target, "new content").unwrap();

        assert_eq!(fs::read_to_string(target).unwrap(), "new content");
    }

    #[test]
    fn atomic_write_replaces_an_existing_file() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("config.json");
        fs::write(&target, "old content").unwrap();

        write_atomic_file(&target, "replacement").unwrap();

        assert_eq!(fs::read_to_string(target).unwrap(), "replacement");
    }

    #[test]
    fn failed_persist_keeps_target_and_cleans_temporary_file() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("config.json");
        fs::create_dir(&target).unwrap();

        assert!(write_atomic_file(&target, "content").is_err());
        assert!(target.is_dir());
        assert_eq!(
            fs::read_dir(directory.path())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".config.json.aimonitor-")
                })
                .count(),
            0
        );
    }

    #[test]
    fn concurrent_writes_never_leave_partial_content() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("config.json");
        let first_content = "a".repeat(128 * 1024);
        let second_content = "b".repeat(128 * 1024);
        let barrier = Arc::new(Barrier::new(3));

        let writers = [first_content.clone(), second_content.clone()].map(|content| {
            let barrier = Arc::clone(&barrier);
            let target = target.clone();
            thread::spawn(move || {
                barrier.wait();
                write_atomic_file(&target, &content).unwrap();
            })
        });
        barrier.wait();
        for writer in writers {
            writer.join().unwrap();
        }

        let persisted = fs::read_to_string(target).unwrap();
        assert!(persisted == first_content || persisted == second_content);
    }
}
