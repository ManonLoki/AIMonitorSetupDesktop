// 配置文件的读写工具：读取一个可能不存在的文件、原子写入文件内容。
// 供 `service_lifecycle.rs` 里持久化配置/Hook 文件的逻辑使用。
use std::{fs, io::Write, path::Path};

use tempfile::Builder;

// 读取一个可能尚不存在的配置文件；文件缺失视为正常情况（返回 None），
// 而不是错误——首次写入前，Hook 配置和应用存储文件都可能还不存在。
pub(super) fn read_optional_config(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("无法读取 {}：{error}", path.display())),
    }
}

// 写入 Hook 配置文件，复用通用的原子写入逻辑。
pub(super) fn write_config(path: &Path, content: &str) -> Result<(), String> {
    write_atomic_file(path, content, "Hooks 配置")
}

// 原子写入文件：先写临时文件，再重命名/替换为目标文件，避免写入过程中崩溃导致文件损坏或内容截断。
pub(super) fn write_atomic_file(path: &Path, content: &str, label: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("无法确定 {} 的配置目录", path.display()))?;
    // 确保目标目录存在。
    fs::create_dir_all(parent)
        .map_err(|error| format!("无法创建配置目录 {}：{error}", parent.display()))?;

    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("配置文件路径无效：{}", path.display()))?;
    // 随机临时文件必须与目标文件位于同一目录，才能由 persist 在各平台执行
    // 原子替换；NamedTempFile 在任何失败路径上都会自动清理尚未持久化的文件。
    let temporary_prefix = format!(".{filename}.aimonitor-");
    let mut temporary_file = Builder::new()
        .prefix(&temporary_prefix)
        .suffix(".tmp")
        .tempfile_in(parent)
        .map_err(|error| format!("无法在 {} 创建临时配置：{error}", parent.display()))?;
    let temporary_path = temporary_file.path().to_path_buf();

    temporary_file
        .as_file_mut()
        .write_all(content.as_bytes())
        .map_err(|error| format!("无法写入临时配置 {}：{error}", temporary_path.display()))?;
    temporary_file
        .as_file()
        .sync_all()
        .map_err(|error| format!("无法同步临时配置 {}：{error}", temporary_path.display()))?;
    temporary_file
        .persist(path)
        .map_err(|error| format!("无法写入{label} {}：{}", path.display(), error.error))?;

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

        write_atomic_file(&target, "new content", "测试配置").unwrap();

        assert_eq!(fs::read_to_string(target).unwrap(), "new content");
    }

    #[test]
    fn atomic_write_replaces_an_existing_file() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("config.json");
        fs::write(&target, "old content").unwrap();

        write_atomic_file(&target, "replacement", "测试配置").unwrap();

        assert_eq!(fs::read_to_string(target).unwrap(), "replacement");
    }

    #[test]
    fn failed_persist_keeps_target_and_cleans_temporary_file() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("config.json");
        fs::create_dir(&target).unwrap();

        assert!(write_atomic_file(&target, "content", "测试配置").is_err());
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
                write_atomic_file(&target, &content, "测试配置").unwrap();
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
