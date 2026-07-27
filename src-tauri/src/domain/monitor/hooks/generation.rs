use std::path::Path;

use serde_json::{Map, Value, json};

use super::{
    AiTool, HookConfigPreview, HookProtocol, ManagedCommands, managed_hook_marker, protocol,
    shell_quote,
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

// 判断一条当前格式的命令字符串是否携带指定管理标识。Windows PowerShell
// 宿主可能保留用于保护双引号的反引号，因此比较前只归一化该现役转义形式。
pub(super) fn command_has_marker(command: &str, marker: &str) -> bool {
    let contains_marker = |value: &str| {
        value.contains(&format!("{marker}'")) || value.contains(&format!("{marker}\""))
    };
    contains_marker(command) || contains_marker(&command.replace("`\"", "\""))
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_merge;
