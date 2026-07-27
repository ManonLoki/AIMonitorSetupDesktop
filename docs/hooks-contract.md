# Hooks 事实标准

本文定义 AIMonitor 当前唯一受支持的 Hooks 契约。实现、测试与文档必须以本契约
为准，不提供旧标识、旧命令、旧工具名称或旧配置结构的识别与迁移。

## 权威实现

- 工具元数据、事件表与配置结构：`src-tauri/src/domain/monitor/hooks/`
- 配置生成与合并：`src-tauri/src/domain/monitor/hooks/generation.rs`
- 原生输入归约：`src-tauri/src/domain/monitor/payload.rs`
- 事件状态机：`src-tauri/src/domain/monitor/hook_state_machine/`
- 设备目标选择与转发：`src-tauri/src/application/monitor/relay/`

协议调整必须同时修改对应工具实现与生成测试。禁止在公共生成器、listener 或
application 层按工具硬编码另一份事件表。

## 受支持工具

| 工具 | slug | 默认主配置 | 接入形式 | 写入后要求 |
| --- | --- | --- | --- | --- |
| Codex | `codex` | `~/.codex/hooks.json` | command Hook | 审核，并重启或新建任务 |
| Claude Code | `claude-code` | `~/.claude/settings.json` | command Hook | 无 |
| Cursor | `cursor` | `~/.cursor/hooks.json` | command Hook | 无 |
| OpenCode | `opencode` | `~/.config/opencode/plugins/aimonitor.js` | 原生插件 | 无 |
| WorkBuddy | `workbuddy` | `~/.workbuddy/settings.json` | Git Bash/POSIX command Hook | 审核，并重启或新建会话 |
| Hermes | `hermes` | `~/.hermes/plugins/aimonitor/__init__.py` | 原生 observer 插件 | 启用插件，并重启或新建会话 |
| OpenClaw | `openclaw` | `~/.openclaw/extensions/aimonitor/index.mjs` | 原生插件 | 启用并授权插件，重启 Gateway |
| CodeBuddy | `codebuddy` | `~/.codebuddy/settings.json` | Git Bash/POSIX command Hook | 审核，并重启或新建会话 |

Hermes 的工具名与目录字段只接受 `hermes`；不存在其他别名。OpenCode、Hermes、
OpenClaw 的辅助 manifest/metadata 与主入口视为一组，任一现有文件不是当前
AIMonitor 管理的文件时整组拒绝覆盖。

## 管理标识与命令

- 唯一管理标识为 `AIMonitor:tool={slug}`，大小写与 `:` 分隔符固定。
- command Hook 只调用当前安装的 `AIMonitor --aimonitor-hook-relay`。
- 不生成或识别 PowerShell/curl 直传、编码 PowerShell、runner 脚本或其他标识。
- 合并只替换带当前管理标识的条目；其他内容作为用户配置原样保留。
- 应用启动不扫描、不重写 Hooks 文件。需要更新时由用户在“Hooks 管理”明确执行写入。

## 本机 listener 契约

- 地址固定为 `POST http://127.0.0.1:10240/api/hooks/{slug}`。
- `Content-Type` 为 `application/json`。
- `X-AIMonitor-Hook-Type` 必须存在，并与正文 `hook_event_name` 完全一致。
- 正文只允许以下字段：

```json
{
  "hook_event_name": "事件名",
  "session_id": "可选",
  "turn_id": "可选",
  "status": "可选"
}
```

正文上限为 4 KiB，并拒绝未知字段。command relay 可从各工具当前原生输入中提取
等价的会话/轮次字段，但 prompt、transcript、tool input/output 等内容不得进入
listener。

## 状态与投递

- 各工具支持的精确事件名及其 `Idle`、`Running`、`Asking`、`Error`、`Release`
  映射，以对应 `hooks/<tool>.rs` 的 `EVENTS` 为唯一事件表。
- Codex、Claude Code、Cursor、OpenCode 使用会话/轮次状态机和 latest-wins
  目标队列；WorkBuddy、Hermes、OpenClaw、CodeBuddy 按事件 FIFO 直通。
- 一次状态转换的候选目标必须同时满足：设备存在已保存路由、该工具存在 Profile、
  设备 ID 位于当前在线快照。
- 每次槽位 POST 必须携带本控制端稳定的 `clientId`。后台每 30 秒向同一在线目标调用
  `/api/clients/{clientId}/heartbeat`；接收端 2 分钟未收到续租时按 DELETE 等价语义
  清理该客户端拥有的全部槽位。该租约流量不计入 Hook 中继统计。
- 入队前先与在线快照求交集；目标 worker 真正发送 HTTP 前再次确认设备仍在线。
  已记录但不在线、或排队期间离线的设备直接跳过，不使用历史地址尝试转发，也不
  计为网络失败。
- 在线目标始终使用发现快照中的最新名称与地址。单台在线设备失败不阻止其他设备。

## 变更门禁

`domain::monitor::hooks::generation::tests` 固化所有工具的 slug、事件、管理标识和
生成结构；`application::monitor::relay` 测试固化在线目标过滤及发送前复检。任何
契约变更必须更新这些测试与本文，并通过 `pnpm check`。
