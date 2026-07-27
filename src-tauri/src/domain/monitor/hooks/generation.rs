use std::path::Path;

use serde_json::{Map, Value, json};

use super::{
    AiTool, HookConfigPreview, HookProtocol, MANAGED_HOOK_PREFIX, ManagedCommands,
    contains_managed_marker, managed_hook_marker, protocol, shell_quote,
};

// 为一个工具生成完整的主配置文件内容：独立配置文件路线的工具直接返回其
// `standalone_config`；否则按该工具声明的事件表逐个生成 handler 并组装。
pub fn generate_hook_config(
    tool: AiTool,
    relay_executable: &Path,
) -> Result<HookConfigPreview, String> {
    let protocol = protocol(tool);
    if let Some(content) = protocol.standalone_config() {
        return Ok(HookConfigPreview {
            filename: protocol.preview_filename().to_owned(),
            content,
        });
    }
    let mut hooks = Map::new();

    for event in protocol.events() {
        let commands = managed_commands(protocol, event.name, relay_executable);
        hooks.insert(event.name.to_owned(), protocol.handler(event, &commands));
    }

    let config = protocol.config_root(hooks);
    Ok(HookConfigPreview {
        filename: protocol.preview_filename().to_owned(),
        content: serde_json::to_string_pretty(&config)
            .map_err(|error| format!("无法生成 Hooks 配置：{error}"))?,
    })
}

// 把新生成的配置与现有文件合并：先移除现有文件中该工具的旧受管条目，
// 再插入新生成的条目，用户手动添加的其他事件/命令原样保留。
pub fn merge_hook_config(
    existing_content: Option<&str>,
    generated: &HookConfigPreview,
    tool: AiTool,
) -> Result<HookConfigPreview, String> {
    let protocol = protocol(tool);
    if protocol.standalone_config().is_some() {
        return Ok(HookConfigPreview {
            filename: generated.filename.clone(),
            content: protocol.merge_standalone(existing_content, generated)?,
        });
    }
    let mut existing = match existing_content {
        Some(content) => serde_json::from_str::<Value>(content)
            .map_err(|error| format!("现有 Hooks 配置格式错误：{error}"))?,
        None => json!({}),
    };
    let generated_value = serde_json::from_str::<Value>(&generated.content)
        .map_err(|error| format!("生成的 Hooks 配置格式错误：{error}"))?;
    let existing_root = existing
        .as_object_mut()
        .ok_or_else(|| "现有 Hooks 配置的根节点必须是对象".to_owned())?;
    let generated_root = generated_value
        .as_object()
        .ok_or_else(|| "生成的 Hooks 配置的根节点必须是对象".to_owned())?;

    for (key, value) in generated_root {
        if key != "hooks" {
            existing_root
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }

    let existing_hooks = existing_root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| "现有配置中的 hooks 必须是对象".to_owned())?;
    let generated_hooks = generated_root
        .get("hooks")
        .and_then(Value::as_object)
        .ok_or_else(|| "生成的配置缺少 hooks 对象".to_owned())?;
    for event in existing_hooks.keys().cloned().collect::<Vec<_>>() {
        let should_remove = existing_hooks.get_mut(&event).is_some_and(|entries| {
            let Some(entries) = entries.as_array_mut() else {
                return false;
            };
            protocol.remove_managed_entries(entries);
            entries.is_empty()
        });
        if should_remove {
            existing_hooks.remove(&event);
        }
    }

    for (event, generated_entries) in generated_hooks {
        let generated_entries = generated_entries
            .as_array()
            .ok_or_else(|| format!("生成的 {event} 配置必须是数组"))?;
        let existing_entries = existing_hooks
            .entry(event.clone())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| format!("现有配置中的 {event} 必须是数组"))?;
        existing_entries.extend(generated_entries.iter().cloned());
    }

    Ok(HookConfigPreview {
        filename: generated.filename.clone(),
        content: serde_json::to_string_pretty(&existing)
            .map_err(|error| format!("无法生成合并后的 Hooks 配置：{error}"))?,
    })
}

/// 判断现有配置中是否包含指定工具的 `AIMonitor` 受管条目。
///
/// 启动时的升级修复只允许重写已经由 `AIMonitor` 管理的配置；用户自己的 Hook
/// 文件即使位于默认目录，也不能因为应用升级而被自动加入新条目。
pub fn contains_managed_hook_config(existing_content: &str, tool: AiTool) -> bool {
    let protocol = protocol(tool);
    if protocol.standalone_config().is_some() {
        return contains_managed_marker(existing_content, tool);
    }
    let Ok(existing) = serde_json::from_str::<Value>(existing_content) else {
        return false;
    };
    existing
        .get("hooks")
        .and_then(Value::as_object)
        .is_some_and(|hooks| {
            hooks.values().any(|entries| {
                let Some(entries) = entries.as_array() else {
                    return false;
                };
                let mut remaining = entries.clone();
                protocol.remove_managed_entries(&mut remaining);
                remaining != *entries
            })
        })
}

// 为一个事件生成三种平台变体的托管命令字符串（POSIX shell、Windows CMD、
// 经 PowerShell 转发到 CMD），供各工具的 `handler` 按自身协议组装配置条目。
fn managed_commands(
    protocol: &dyn HookProtocol,
    event: &str,
    relay_executable: &Path,
) -> ManagedCommands {
    let marker = managed_hook_marker(protocol.tool());
    let executable = relay_executable.to_string_lossy().into_owned();
    let posix_executable = if cfg!(windows) {
        executable.replace('\\', "/")
    } else {
        executable.clone()
    };
    let posix = format!(
        "{} --aimonitor-hook-relay {} {} --managed-by {}",
        shell_quote(&posix_executable),
        shell_quote(protocol.slug()),
        shell_quote(event),
        shell_quote(&marker),
    );
    let windows = format!(
        "cmd.exe /d /s /c \"{} --aimonitor-hook-relay {} {} --managed-by {}\"",
        windows_quote(&executable),
        windows_quote(protocol.slug()),
        windows_quote(event),
        windows_quote(&marker),
    );
    let windows_powershell_host = format!(
        "cmd.exe /d /s /c \"{} --aimonitor-hook-relay {} {} --managed-by {}\"",
        powershell_host_quote(&executable),
        powershell_host_quote(protocol.slug()),
        powershell_host_quote(event),
        powershell_host_quote(&marker),
    );
    ManagedCommands {
        posix,
        windows,
        windows_powershell_host,
    }
}

// 按 Windows CMD 双引号规则转义一个参数。
fn windows_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

// PowerShell 会先解析整条 command，再把 `/c` 后的文本交给 CMD。用反引号保护
// 内层双引号，才能同时保住含空格的安装路径和参数边界。
fn powershell_host_quote(value: &str) -> String {
    format!("`\"{}`\"", value.replace('`', "``").replace('"', "`\""))
}

// 判断一条已写入配置的命令字符串是否携带指定的管理标识；除了直接文本匹配，
// 还要能识别被 PowerShell `-EncodedCommand` 编码过的旧版本命令。
pub(super) fn command_has_marker(command: &str, marker: &str) -> bool {
    let contains_marker = |value: &str| {
        value.contains(&format!("{marker}'")) || value.contains(&format!("{marker}\""))
    };
    contains_marker(command)
        || contains_marker(&command.replace("`\"", "\""))
        || contains_marker(&command.replace("^`|", "|"))
        || decoded_hook_command(command)
            .as_deref()
            .is_some_and(contains_marker)
}

// 若命令已是明文则原样返回；否则尝试从 `-EncodedCommand <base64>` 参数中解码出
// PowerShell 用的 UTF-16LE 编码原文，用于兼容旧版本生成的托管命令。
fn decoded_hook_command(command: &str) -> Option<String> {
    if command.contains(MANAGED_HOOK_PREFIX) {
        return Some(command.to_owned());
    }
    let encoded = command
        .split_once("-EncodedCommand ")
        .map(|(_, encoded)| encoded.split_whitespace().next().unwrap_or(""))?;
    let bytes = decode_base64(encoded)?;
    if bytes.len() % 2 != 0 {
        return None;
    }
    let utf16 = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    String::from_utf16(&utf16).ok()
}

// 标准 Base64 解码（自实现，配套 `encode_base64`），非法字符或长度直接返回 None。
fn decode_base64(value: &str) -> Option<Vec<u8>> {
    // 把一个 Base64 字符映射回它代表的 6 位数值。
    fn sextet(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = value.as_bytes();
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut decoded = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks_exact(4) {
        let a = sextet(chunk[0])?;
        let b = sextet(chunk[1])?;
        let c = (chunk[2] != b'=').then(|| sextet(chunk[2])).flatten();
        let d = (chunk[3] != b'=').then(|| sextet(chunk[3])).flatten();
        decoded.push((a << 2) | (b >> 4));
        if let Some(c) = c {
            decoded.push((b << 4) | (c >> 2));
            if let Some(d) = d {
                decoded.push((c << 6) | d);
            }
        }
    }
    Some(decoded)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_merge;
