# AI 工具覆盖与竞品迭代路线

最后核验：2026-07-29。

本文记录第二阶段覆盖决策、暂缓工具和 GitHub 同类项目可借鉴能力。它不替代
[`hooks-contract.md`](hooks-contract.md)；已经接入的精确事件映射仍以 Rust
`domain/monitor/hooks/<tool>.rs` 为唯一事实来源。

## 接入门槛

一个工具进入“正式支持”前必须同时满足：

1. 官方公开用户级 Hook、插件或 observer 契约，不能依赖逆向私有数据库。
2. 至少能稳定表达工作开始和停止；缺少权限或错误事件时必须明确标注降级。
3. 配置位置、事件名、stdin/stdout、退出码和 Windows shell 行为均可验证。
4. AIMonitor 只提取最小会话字段，不转发 prompt、transcript、tool input/output。
5. 生成、合并、幂等、损坏边界和状态转换均有 Rust 测试。

文件监听或日志推断可以作为未来的“实验性适配”，但不得冒充官方 Hook。UI 和
诊断输出需要明确区分 `native`、`compatibility subset` 与 `inferred` 三种能力等级。

## 第二阶段已落地

| 工具 | 覆盖方式 | 兼容性边界 | 官方依据 |
| --- | --- | --- | --- |
| Qwen Code | `~/.qwen/settings.json` command Hooks | 生命周期、权限、失败与子代理；成功 stdout 返回空 JSON | [Hooks](https://github.com/QwenLM/qwen-code/blob/main/docs/users/features/hooks.md) |
| Kimi Code | `($KIMI_CODE_HOME 或 ~/.kimi-code)/config.toml` TOML command Hooks | 14 个确定性四态事件；Windows 使用 Git Bash；共享配置只替换托管区块 | [Hooks](https://moonshotai.github.io/kimi-code/en/customization/hooks)、[配置](https://moonshotai.github.io/kimi-code/en/configuration/config-files.html) |
| Qoder | `~/.qoder/settings.json` command Hooks | 采用 IDE、JetBrains 与 CLI 都支持的五事件兼容基线；配置立即生效 | [跨端基线](https://docs.qoder.com/extensions/hooks)、[CLI 扩展事件](https://docs.qoder.com/en/cli/hooks) |
| Gemini CLI | `~/.gemini/settings.json` command Hooks | Agent、Model、Tool、Session、压缩与权限通知；stdout 严格 JSON | [Hooks](https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/index.md) |
| GitHub Copilot CLI | `~/.copilot/hooks/aimonitor.json` 扁平 v1 command Hooks | 用户级独立文件；`preToolUse` relay 故障 fail-open，避免旁路监控阻断工具 | [参考](https://docs.github.com/en/copilot/reference/hooks-reference) |

Qoder 的 CLI 契约比 IDE/JB 插件更丰富。当前共享用户配置选择公共子集，避免
CLI 专属事件让其他入口拒绝或误读配置；后续应通过“工具 + 运行入口”能力档案拆分。

## 国产工具观察清单

| 优先级 | 工具 | 当前结论 | 进入实现前还缺什么 |
| --- | --- | --- | --- |
| P1 | Baidu Comate | 2026-04 的 IDE/插件更新已宣布 Hooks，2026-06 又加入插件级 Hook 加载 | 官方完整事件表、用户级路径、handler schema、退出码和跨平台 shell 契约 |
| P2 | TRAE / Trae Agent | 开源 Agent 提供 trajectory 记录，适合离线观测；未验证到稳定用户级生命周期 Hook | TRAE IDE 官方 Hook 文档，或把 trajectory watcher 明确做成实验性推断适配 |
| P2 | 通义灵码 | 有 IDE/CLI 与 Skills 生态，但尚未核验到公开用户级生命周期 Hook | 官方 Hook 配置和事件契约 |
| P2 | CodeGeeX | 公开仓库以模型和 IDE 扩展为主，尚未核验到 Agent 生命周期 Hook | 官方用户扩展点和会话事件 |
| 不接入 | iFlow CLI | 仓库曾提供 Claude-style Hooks，但官方已宣布 2026-04-17 停服 | 无；避免给已终止产品新增维护负担 |

参考：[Comate 更新日志](https://cloud.baidu.com/doc/COMATE/s/2mjzerjsp)、
[TRAE Agent](https://github.com/bytedance/trae-agent)、
[iFlow CLI 停服说明入口](https://github.com/iflow-ai/iflow-cli)。

## GitHub 同类项目对比

| 项目 | 强项 | 相对 AIMonitor 的边界 | 应借鉴能力 |
| --- | --- | --- | --- |
| [PeonPing](https://github.com/PeonPing/peon-ping) | 大量 CLI/IDE adapter、统一事件格式、声音/桌面浮层/手机推送、窗口焦点识别；对无 Hook 工具可用文件 watcher | 部分“支持”是推断事件；核心目标是提醒而非多设备一致状态与租约 | adapter 能力等级、安装探测、配置 doctor、免打扰/焦点策略、可选移动提醒 |
| [Claude Code Multi-Agent Observability](https://github.com/disler/claude-code-hooks-multi-agent-observability) | 12 类事件实时流、WebSocket、SQLite、筛选时间线 | 仅 Claude Code，部署依赖较多，并会采集比四态更多的原始上下文 | 本地事件时间线、Hook 调试页、记录/回放测试，但继续坚持最小字段 |
| [CAST Dashboard](https://github.com/ek33450505/claude-code-dashboard) | Hook 健康、会话/Agent/成本/Token/可靠性、SSE 与本地数据库 | 深度绑定 Claude/CAST，不是跨工具设备状态控制器 | Hook health、协议漂移诊断、失败率和延迟趋势、可导出的本地审计摘要 |
| [OpenHarness](https://github.com/HKUDS/OpenHarness) | Agent runtime 将工具、权限、Hooks、MCP、记忆和多 Agent 作为模块化子系统 | 是新 Agent 运行时，不是现有工具的旁路兼容层；公开 Hook 重点是 Pre/PostToolUse | 以 capability manifest 驱动 adapter，而不是让 UI/业务层硬编码工具差异 |

AIMonitor 当前差异化仍是：跨工具四态归约、并发会话状态机、每设备独立队列、
在线快照与租约、多设备实体展示，以及默认不保存敏感原始 Hook 内容。覆盖数量不应
以牺牲这些一致性和隐私边界为代价。

## 下一轮优先级

### 方向一：AI Hooks 支持和兼容性

1. **P0：能力档案与契约夹具。** 为每个工具声明入口、版本、配置格式、事件、
   stdout、失败策略、热加载和 Windows shell；用脱敏官方 payload fixture 做回归。
2. **P0：Hook Doctor。** 只读检查工具是否安装、配置是否加载、托管区块是否完整、
   最近一次事件是否到达，以及版本是否落在已验证范围。
3. **P1：Baidu Comate。** 官方完整契约可核验后优先加入；不从私有扩展存储猜路径。
4. **P1：入口级适配。** 先拆 Qoder CLI 与 IDE/JB 能力，再复用到 Comate、Kiro 等
   同一品牌多入口工具。
5. **P2：实验性 watcher。** TRAE 等无 Hook 工具仅在用户显式开启后从公开轨迹推断
   Running/Idle，并在 UI 标记“推断”，不映射 Asking/Error 等无法证明的状态。

### 方向二：其他功能

1. **P0：安全预览、备份与回滚。** 写入前展示结构化 diff；保留最近一次可恢复备份，
   对共享 TOML/JSON 的损坏边界提供一键恢复。
2. **P0：本地诊断时间线。** 只保存工具、事件、归约状态、时间、投递结果和脱敏
   session hash；默认不保存 prompt、工具参数或 transcript。
3. **P1：协议健康与趋势。** 展示事件接收率、Hook 执行失败、listener 延迟、设备投递
   延迟、抑制数，并给出“配置未生效/版本漂移/设备离线”的可执行诊断。
4. **P1：通知路由。** 对 Asking/Error/Stop 提供桌面与可选移动通知，支持工作时间、
   系统专注模式、当前窗口聚焦和每工具静音；外部推送凭据只由 Rust 安全存储。
5. **P2：并发会话视图。** 在不改变实体屏四态的前提下，显示每工具活动会话数、
   子代理数和最近项目，解决同一工具多个终端时“一个标签看不出谁在等”的问题。

每轮新增工具都应先更新本表的证据，再实现 adapter，最后运行 `pnpm check`、
`pnpm build` 与 `pnpm tauri build`。发布前还需在对应工具的真实 macOS、Windows
或 WSL 进程上做至少一次端到端 smoke test。
