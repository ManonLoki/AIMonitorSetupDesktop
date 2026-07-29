// Windows 宿主访问 WSL 内 Hooks 配置的适配层。UNC 路径只用于识别发行版与
// Linux 路径；实际读写通过 wsl.exe 在对应发行版内完成，避免依赖易失效的 9P UNC 挂载。
use std::path::Path;

#[cfg(target_os = "windows")]
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WslDirectory {
    distribution: String,
    linux_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WslFile {
    distribution: String,
    linux_path: String,
}

impl WslDirectory {
    /// 识别 Windows 为 WSL 文件系统暴露的两种 UNC 前缀。
    pub(super) fn parse(directory: &str) -> Option<Self> {
        let normalized = directory.replace('/', "\\");
        let lowercase = normalized.to_ascii_lowercase();
        let prefix_length = ["\\\\wsl.localhost\\", "\\\\wsl$\\"]
            .into_iter()
            .find(|prefix| lowercase.starts_with(prefix))?
            .len();
        let mut components = normalized[prefix_length..]
            .split('\\')
            .filter(|component| !component.is_empty());
        let distribution = components.next()?.to_owned();
        let remainder = components.collect::<Vec<_>>();
        if remainder
            .iter()
            .any(|component| matches!(*component, "." | ".."))
        {
            return None;
        }
        let linux_path = format!("/{}", remainder.join("/"));
        Some(Self {
            distribution,
            linux_path,
        })
    }

    pub(super) fn join(&self, relative_path: &str) -> WslFile {
        let relative_path = relative_path.replace('\\', "/");
        WslFile {
            distribution: self.distribution.clone(),
            linux_path: format!(
                "{}/{}",
                self.linux_path.trim_end_matches('/'),
                relative_path.trim_start_matches('/')
            ),
        }
    }

    #[cfg(target_os = "windows")]
    pub(super) fn translate_windows_executable(&self, executable: &Path) -> Result<String, String> {
        let executable = executable
            .to_str()
            .ok_or_else(|| "AIMonitor 可执行文件路径不是有效 Unicode".to_owned())?;
        let output = run_wsl(&self.distribution, wslpath_arguments(executable))?;
        let translated = String::from_utf8(output)
            .map_err(|error| format!("WSL 返回了无效的路径编码：{error}"))?;
        let translated = translated.trim();
        if translated.is_empty() || !translated.starts_with('/') {
            return Err(format!(
                "WSL 无法转换 AIMonitor 可执行文件路径：{executable}"
            ));
        }
        Ok(translated.to_owned())
    }

    #[cfg(not(target_os = "windows"))]
    pub(super) fn translate_windows_executable(
        &self,
        _executable: &Path,
    ) -> Result<String, String> {
        let _ = self;
        Err("WSL Hooks 配置只支持 Windows 宿主".to_owned())
    }
}

// wslpath 是 WSL 自带工具，不接受常见 GNU 工具用于终止选项解析的 `--`。
// 将参数集中在这里，确保所有发行版都使用官方支持的调用形式。
#[cfg(any(target_os = "windows", test))]
fn wslpath_arguments(executable: &str) -> [&str; 3] {
    ["wslpath", "-u", executable]
}

impl WslFile {
    pub(super) fn display(&self) -> &str {
        &self.linux_path
    }

    #[cfg(target_os = "windows")]
    pub(super) fn read_optional(&self) -> Result<Option<String>, String> {
        let output = Command::new("wsl.exe")
            .args([
                "-d",
                &self.distribution,
                "--",
                "cat",
                "--",
                &self.linux_path,
            ])
            .output()
            .map_err(|error| wsl_launch_error(&self.distribution, &error))?;
        if output.status.success() {
            return String::from_utf8(output.stdout)
                .map(Some)
                .map_err(|error| format!("WSL Hooks 配置不是有效 UTF-8：{error}"));
        }

        let existence = Command::new("wsl.exe")
            .args([
                "-d",
                &self.distribution,
                "--",
                "test",
                "-e",
                &self.linux_path,
            ])
            .output()
            .map_err(|error| wsl_launch_error(&self.distribution, &error))?;
        if existence.status.code() == Some(1) && existence.stderr.is_empty() {
            return Ok(None);
        }
        Err(wsl_command_error(
            &self.distribution,
            "读取 Hooks 配置",
            &output.stderr,
        ))
    }

    #[cfg(not(target_os = "windows"))]
    pub(super) fn read_optional(&self) -> Result<Option<String>, String> {
        let _ = self;
        Err("WSL Hooks 配置只支持 Windows 宿主".to_owned())
    }

    #[cfg(target_os = "windows")]
    pub(super) fn write_atomic(&self, content: &str) -> Result<(), String> {
        let parent = self
            .linux_path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .filter(|parent| !parent.is_empty())
            .ok_or_else(|| format!("无法确定 WSL 配置目录：{}", self.linux_path))?;
        run_wsl(&self.distribution, ["mkdir", "-p", "--", parent])?;

        let temporary_path = format!("{}.aimonitor.tmp", self.linux_path);
        let mut child = Command::new("wsl.exe")
            .args(["-d", &self.distribution, "--", "tee", &temporary_path])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| wsl_launch_error(&self.distribution, &error))?;
        child
            .stdin
            .take()
            .ok_or_else(|| "无法打开 WSL 配置写入通道".to_owned())?
            .write_all(content.as_bytes())
            .map_err(|error| format!("无法向 WSL 写入临时 Hooks 配置：{error}"))?;
        let output = child
            .wait_with_output()
            .map_err(|error| format!("无法等待 WSL Hooks 配置写入：{error}"))?;
        if !output.status.success() {
            return Err(wsl_command_error(
                &self.distribution,
                "写入临时 Hooks 配置",
                &output.stderr,
            ));
        }

        if let Err(error) = run_wsl(
            &self.distribution,
            ["mv", "-f", "--", &temporary_path, &self.linux_path],
        ) {
            let _ = run_wsl(&self.distribution, ["rm", "-f", "--", &temporary_path]);
            return Err(error);
        }
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub(super) fn write_atomic(&self, _content: &str) -> Result<(), String> {
        let _ = self;
        Err("WSL Hooks 配置只支持 Windows 宿主".to_owned())
    }
}

#[cfg(target_os = "windows")]
fn run_wsl<const N: usize>(distribution: &str, command: [&str; N]) -> Result<Vec<u8>, String> {
    let output = Command::new("wsl.exe")
        .args(["-d", distribution, "--"])
        .args(command)
        .output()
        .map_err(|error| wsl_launch_error(distribution, &error))?;
    if !output.status.success() {
        return Err(wsl_command_error(
            distribution,
            "执行 WSL 文件操作",
            &output.stderr,
        ));
    }
    Ok(output.stdout)
}

#[cfg(target_os = "windows")]
fn wsl_launch_error(distribution: &str, error: &std::io::Error) -> String {
    format!("无法启动 WSL 发行版 {distribution}：{error}")
}

#[cfg(target_os = "windows")]
fn wsl_command_error(distribution: &str, action: &str, stderr: &[u8]) -> String {
    let detail = String::from_utf8_lossy(stderr);
    let detail = detail.trim();
    if detail.is_empty() {
        format!("{action}失败（WSL 发行版：{distribution}）")
    } else {
        format!("{action}失败（WSL 发行版：{distribution}）：{detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::{WslDirectory, wslpath_arguments};

    #[test]
    fn invokes_wslpath_without_an_unsupported_option_separator() {
        assert_eq!(
            wslpath_arguments(r"C:\Users\user\AppData\Local\AIMonitor\AIMonitor.exe"),
            [
                "wslpath",
                "-u",
                r"C:\Users\user\AppData\Local\AIMonitor\AIMonitor.exe"
            ]
        );
    }

    #[test]
    fn recognizes_supported_wsl_unc_paths() {
        let localhost =
            WslDirectory::parse(r"\\wsl.localhost\archlinux\home\leiyut\.claude").unwrap();
        assert_eq!(localhost.distribution, "archlinux");
        assert_eq!(localhost.linux_path, "/home/leiyut/.claude");
        assert_eq!(
            localhost.join("settings.json").display(),
            "/home/leiyut/.claude/settings.json"
        );

        let legacy = WslDirectory::parse(r"\\wsl$\Ubuntu-24.04\home\user\.codex").unwrap();
        assert_eq!(legacy.distribution, "Ubuntu-24.04");
        assert_eq!(legacy.linux_path, "/home/user/.codex");
    }

    #[test]
    fn leaves_normal_windows_paths_on_the_native_path() {
        assert!(WslDirectory::parse(r"C:\Users\user\.claude").is_none());
        assert!(WslDirectory::parse(r"\\server\share\.claude").is_none());
        assert!(WslDirectory::parse(r"relative\.claude").is_none());
    }

    #[test]
    fn rejects_parent_traversal_in_wsl_unc_paths() {
        assert!(WslDirectory::parse(r"\\wsl.localhost\Ubuntu\home\user\..\root").is_none());
    }
}
