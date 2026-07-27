// 配置文件的读写工具：读取一个可能不存在的文件、原子写入文件内容。
// 供 `service_lifecycle.rs` 里持久化配置/Hook 文件的逻辑使用。
use std::{fs, path::Path};

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
    // 临时文件名以 . 开头并带特殊后缀，避免与正常文件冲突或被误读。
    let temporary_path = parent.join(format!(".{filename}.aimonitor.tmp"));
    fs::write(&temporary_path, content)
        .map_err(|error| format!("无法写入临时配置 {}：{error}", temporary_path.display()))?;

    // 类 Unix 系统上 rename 是原子操作，可以安全地替换目标文件；
    // Windows 上 rename 到已存在文件会失败，因此改用直接写入+删除临时文件的方式。
    #[cfg(not(windows))]
    let replace_result = fs::rename(&temporary_path, path);
    #[cfg(windows)]
    let replace_result = fs::write(path, content).and_then(|()| fs::remove_file(&temporary_path));

    if let Err(error) = replace_result {
        // 替换失败时尽力清理临时文件，避免遗留垃圾文件。
        let _ = fs::remove_file(&temporary_path);
        return Err(format!("无法写入{label} {}：{error}", path.display()));
    }
    Ok(())
}
