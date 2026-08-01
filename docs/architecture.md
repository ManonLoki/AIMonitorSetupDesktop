# 架构与代码边界

## 核心原则

Rust 是唯一业务后端，React 是薄 UI。只要逻辑表达了业务含义——包括校验、
授权、转换、聚合、排序、策略、持久化、系统访问和远端通信——就必须在
Rust 中实现。前端只负责渲染、用户交互、页面组织、调用 command 和展示
加载/失败/成功状态。

```text
React page
  ↓ 使用
TanStack Query option / hook
  ↓ 调用
typed TypeScript API
  ↓ invoke
Tauri command（传输适配）
  ↓ 调用
Rust domain / application logic
```

依赖只能沿箭头向下。Rust domain 不依赖 Tauri 或前端概念。

## 目录职责

```text
src/
├── app/                       # Provider、Router、QueryClient 组装
├── features/<feature>/
│   ├── api/                   # command 名称与传输 DTO 类型
│   ├── queries/               # Query key、缓存与失效策略
│   ├── pages/                 # Mantine 页面组合和交互
│   ├── components/            # feature 内可复用的展示组件
│   └── hooks/                 # feature 内可复用的纯 UI 状态逻辑
└── shared/
    ├── state/                 # 仅纯 UI Jotai atoms
    ├── tauri/                 # 通用 invoke 传输适配
    └── ui/                    # 跨 feature 的展示组件

src-tauri/src/
├── commands/                  # Tauri 参数/结果适配；保持极薄
├── domain/                    # 业务实体、规则与纯逻辑
│   └── monitor/hooks/         # HookProtocol 公共契约及各 AI 工具独立协议实现
├── application/                # 服务编排（如 MonitorService、设备发现）
└── lib.rs                     # 插件与 command 注册
```

业务增长后可以在 Rust 继续增加 `infrastructure/` 等新层，但不得把业务
逻辑放回 command 或 React。

## 状态归属

| 状态类型 | 归属 | 示例 |
| --- | --- | --- |
| 业务事实 | Rust | 监控配置、状态判断、告警规则 |
| 异步调用结果 | TanStack Query | command 返回值、加载状态、缓存 |
| URL 可表达状态 | TanStack Router | 当前页面、筛选参数、可分享选择 |
| 短生命周期 UI 状态 | Jotai / 组件本地状态 | 主题、界面语言、侧栏开关、未提交输入 |

禁止把 Rust 返回的数据复制进 Jotai。这样会产生双重事实来源，也绕过 Query
的缓存失效模型。

界面国际化由 `src/shared/i18n/` 的类型化资源统一提供。语言偏好属于本机纯 UI
状态，使用 Jotai 持久化；默认值为 `system`，根据系统语言在简体中文与英文间
解析，也允许用户在设置页显式固定语言。业务错误仍由 Rust 返回，前端不得根据
自由文本反推业务状态。

## Command 合约

新增功能按以下顺序实现：

1. 在 Rust domain/application 中实现并测试用例。
2. 在 `commands/` 添加只做反序列化、调用和错误映射的 adapter。
3. 在 `lib.rs` 注册 command。
4. 在 feature 的 `api/` 声明对应 TypeScript DTO 和调用函数。
5. 在 `queries/` 建立稳定 query key 或 mutation。
6. 页面只组合 Mantine 组件并处理 UI 状态。

Rust DTO 使用 `serde` 的 `camelCase` 输出以匹配 TypeScript。Command
失败必须返回可序列化、对 UI 有意义的错误；不要在前端解析自由文本来推断
业务状态。

监控功能当前使用以下聚合契约保持 React 薄层：

- `get_monitor_capabilities` 从 Rust 领域常量和 `HookProtocol` 返回有序工具目录、
  Hook 行为、发现间隔/Profile 槽位范围与图片上传格式策略；前端只把这些值用于
  控件渲染。
- `discover_monitor_devices`、`select_monitor_device` 与
  `monitor-devices-changed` 统一返回 `MonitorDeviceSnapshot`。在线列表、当前设备、
  其他设备、离线保存路由与可用性标记必须由 Rust 在同一事务锁下生成；选择或
  在线列表实际变化时推进单调 `revision`，前端设备 Query 只接纳同版本或更新快照。
  `hasAvailableDevice` 仅在选中设备确实存在于在线快照时为真。
- `list_ai_profiles` 返回 Rust 为全部工具生成的 `AiProfileDraft`（已有配置覆盖领域
  默认草稿）及读取时的 `expectedDeviceId` 并发令牌；`save_ai_profile` 的草稿不携带
  设备 ID，由应用服务在同一写锁内校验令牌、绑定当前设备并做权威校验和持久化。
  设备已切换时必须拒绝旧草稿，不能把旧设备内容绑定到新设备。
- `list_remote_images` 返回 `RemoteImageGallery`：每张图片携带已验证的显式格式，
  JPEG/PNG/GIF 数量由 Rust 聚合；BMP/WebP 只作为上传输入并由 Rust 转换。前端仅
  保留当前分类选择与列表展示过滤。图片列表、上传和删除同样携带快照中的
  `expectedDeviceId`；Rust 在访问网络前校验当前选择，防止操作跨设备漂移。
- Hook 写入返回 `HookWriteOutcome`，最近中继事件返回带 `kind` 的枚举；React 只将
  结果码映射为本地化文案，不再跨字段或按工具推断业务结果。

## 前端允许与禁止

允许：

- 必填提示、输入格式反馈等即时 UX 校验；Rust 仍需做权威校验。
- 日期/数字的纯展示格式化。
- 页面导航、弹窗、主题和焦点等 UI 行为。
- Query 的加载、重试、缓存和失效设置。

禁止：

- 在 React 中决定业务状态或复刻 Rust 规则。
- 从前端直接访问数据库、文件系统、系统命令或远端业务 API。
- 用 Jotai 保存服务端事实。
- 在组件中散落裸 `invoke`；必须经过 feature `api/` 的类型化函数。
- 为了复用而创建包含业务判断的 TypeScript “service”。

## 参考实现

`get_system_overview` 展示了完整调用链：

- Rust：`src-tauri/src/domain/system.rs`
- Command：`src-tauri/src/commands/system.rs`
- TypeScript API：`src/features/system/api/system.ts`
- Query：`src/features/system/queries/system.ts`
- 消费方：`src/shared/ui/AppShellLayout.tsx`（在应用外壳中展示设备状态）

后续功能应复制这条边界，而不是把实现集中到单个 React 组件。

## Hook 中继与状态归属

AI 工具的 Hooks 不再直接访问监控屏。桌面端是状态计算与转发的唯一事实来源：

```text
Codex / Claude Code / Cursor / WorkBuddy / CodeBuddy / Qwen Code / Kimi Code / Qoder / Gemini CLI / GitHub Copilot CLI command Hook
  ↓ AIMonitor --aimonitor-hook-relay：读取原生 stdin，仅提取必要上下文
OpenCode / Hermes / OpenClaw native plugin
  ↓ 直接构造相同的最小信封
POST 127.0.0.1:10240/api/hooks/{tool}
  ↓ { hook_event_name, session_id?, turn_id?, status? } + 受配置控制的事件头
Rust 本机 Hook listener
  ↓ 根据 tool + Hook type 推进纯 Rust 生命周期状态机
Rust 中读取共享用户名，并将已配置 Profile 与当前在线设备快照求交集
  ↓ 携带持久化 clientId，仅对在线设备 POST / DELETE /api/slots/{slot}
AIMonitor Android / Desktop（单台失败不终止后续设备）
```

控制端首次启动生成并持久化稳定的 `clientId`。槽位 POST 使用该身份声明所有者；
独立后台线程每 30 秒向当前在线且已配置 Profile 的设备调用
`POST /api/clients/{clientId}/heartbeat`。Android/Desktop 维护 2 分钟租约，过期时
只清理该控制端拥有的槽位，清理结果与对应槽位的 DELETE 完全一致。心跳失败不进入
Hook 成功/失败统计，也不阻塞其他设备或状态投递。

- Hooks 的完整唯一契约见 [`hooks-contract.md`](hooks-contract.md)。协议实现与该
  文档共同构成事实标准，不保留历史格式迁移入口。
- 接入候选、竞品比较和后续优先级见
  [`ai-tool-coverage-roadmap.md`](ai-tool-coverage-roadmap.md)；候选表不能替代已实现契约。
- listener 只绑定环回地址，不向局域网暴露 Hook 接口。
- 命令型 Hooks 不依赖 PowerShell、curl 或额外 `.sh`/`.ps1` 文件，而是调用当前
  已安装 AIMonitor 可执行文件的轻量 relay 子命令；该模式在 `main` 入口先行
  截获，不初始化 Tauri、单实例插件或 UI。配置不保存监控屏 IP、用户名、图片或
  状态规则；应用安装路径移动后需重新执行一次 Hooks 写入。
- AI 客户端与桌面端同时冷启动时，Hook 可能早于本机 listener 完成绑定；轻量
  relay 会对本机中继的连接重试 5 次、每次间隔 1 秒。该重试只覆盖工具到本机
  中继的启动竞态，不改变中继到设备“一次转换只投递一次”的规则。GitHub Copilot
  CLI 使用 fail-open：relay 异常不应阻断 CLI 本地命令流程。
- 所有新托管 Hook 命令统一携带大小写固定的 `AIMonitor` 前缀（完整标识如
  `AIMonitor:tool=codex`）。标识不包含 shell 元字符，避免 Windows Hook 宿主在
  `cmd.exe` 前额外经过 PowerShell 时把参数拆成管道。合并、覆盖和后续删除只按
  当前标识识别。
- Windows 原生 command Hook 执行器只写入标准 `cmd.exe` 命令；不同时混入
  POSIX `command` 与 `commandWindows`。WorkBuddy、CodeBuddy、Kimi Code 是例外：
  客户端自身固定通过随产品提供的 Git Bash/POSIX shell 执行 Hook，因此配置直接
  启动 `AIMonitor.exe`，不调用外部脚本、curl 或用户另行安装的 Bash。
- Windows 宿主选择 WSL UNC 配置目录时，由 application 层识别发行版并通过
  `wsl.exe` 完成 Linux 配置读写及 `wslpath` 路径转换，domain 生成 POSIX command；
  普通 Windows 目录继续生成既有 CMD command。WSL 与原生 Windows 是显式分支，
  不通过写入失败后的隐式 fallback 相互切换。
- 应用启动以及设置页保存 AI 客户端多选后，application 层会在独立后台线程中按
  已选择工具逐个执行 best-effort Hooks 自动补写，启动与设置保存不等待文件系统或
  WSL 操作完成。自动流程只检查工具主配置中是否存在当前管理标识：
  已存在则保持整份配置原样，缺失时才调用与手动写入相同的生成、合并和原子写入
  流程。单个工具读取、格式、权限、冲突或 WSL 失败不阻止应用启动、AI 选择保存
  或后续工具；取消选择也不删除既有 Hooks。已有受管规则需要更新 relay 路径或
  协议结构时，用户仍从“Hooks 管理”明确执行手动写入。
- Profile 按“设备 ID + AI 工具”隔离保存显示位置及四种显示状态，不保存
  设备名称或地址。设备名称和地址作为独立设备路由保存在桌面端；显示用户名是
  所有设备共享的全局设置。左下角当前设备只决定监控管理页正在编辑哪台设备。
  编辑页取得的是 Rust 按完整工具目录生成的无设备归属草稿；保存时由应用服务在
  同一写锁内绑定当前设备，前端不得构造默认 Profile 或提交设备归属。
  中继收到 Hook 后，先把所有已保存路由及对应 Profile 与当前在线设备快照求交集，
  仅为在线目标按“设备 ID + AI 工具”投入独立队列并行转发；worker 发送 HTTP 前
  再次确认设备仍在线。离线历史设备不转发也不计失败；单台在线设备失败或超时不
  阻止其余设备，也不延迟其余设备的下一次状态更新。
  未配置的新设备从空 Profile 开始，不继承或转移上一台设备的配置。Hooks
  JSON 仍只连接固定本机中继，因此切换设备时无需重写。
- 监控管理页只保存所选 AI 客户端的 Profile；设置页持久化 AI 客户端多选范围，
  默认启用 Codex、Claude Code、Cursor，并由该范围共同控制监控管理和 Hooks 管理
  的选项卡，同时触发上述缺失 Hooks 自动补写。各 AI 工具配置目录与手动重写/
  更新 Hooks 配置集中在设置页独立的“Hooks 管理”卡片。“工作台”展示 Hook listener、队列及中继统计
  （收到/转发成功/失败/待处理/时序抑制次数，以及最近一次显式
  `display`/`release`/`suppressed` 事件与错误——均为跨设备的聚合计数，不含逐设备明细）；
  只有真实 `Release` 转换可以写入 `release`，无 Profile、调度失败或不支持的 Hook
  只更新失败信息，不得伪造最近事件。工作台同时展示当前在线设备列表（复用发现
  设备的 TanStack Query 缓存），并提供“强制
  重新检查”按钮立即触发一次设备发现，不等待后台定时器。设置页卡片依次为
  “AI 客户端”“Hooks 管理”“通用设置”；通用设置管理共享用户名、
  开机自启、在线设备自动检查间隔及重新触发新手引导。新手引导属于本机持久化
  的纯 UI 状态，首次运行自动显示，完成或跳过后不再自动出现；它先说明侧栏各
  菜单功能与左下角设备切换，再引导用户选择 AI 客户端并理解 Hooks 自动补写，
  最后进入监控管理设置显示位置与四种行为并保存 Profile。引导不再要求进入图片
  管理页：每个行为的图片选择器都可直接上传一张图片，设备返回最终文件名后自动
  选中并写入当前 Profile 草稿；图片管理页继续负责批量上传与图库维护。写入时 Rust 只生成固定本机中继规则，不依赖
  设备 Profile，因此可以先写入 Hooks，再在监控管理中保存展示配置。Hooks
  配置目录仍由 Rust 校验并持久化。用户名缺省时由 Rust 使用本机系统用户名。
  完全没有在线设备时，监控管理和图片管理
  入口禁用，页面也不发起设备业务请求；侧栏明确显示“无可用设备”。
- 命令型 Hook 的轻量 relay 在 HTTP 之前解析工具写入 stdin 的原生 JSON，只保留
  `hook_event_name`、`session_id`、`turn_id`、`status`；同时兼容从原生输入读取
  Cursor 的 `conversation_id`/`generation_id` 与 WorkBuddy 的 camelCase
  `sessionId`/`turnId`，但在线路上统一使用规范字段。候选上下文字段会去除首尾
  空白并跳过空值，避免空的规范字段遮蔽后续有效别名。
  prompt、transcript、tool input/output 等无关或敏感大字段不会进入 listener。
  listener 正文上限为 4 KiB，并以 `deny_unknown_fields` 严格拒绝契约外字段；
  `X-AIMonitor-Hook-Type` 必须存在且必须与正文事件一致。
  事件到状态的归一化、重复事件消除、迟到完成事件抑制和会话释放全部由
  `domain/monitor/hook_state_machine` 的状态机决定。除 Cursor 无轮次 ID 的反向
  End/Start 使用 250ms 交接消歧外，事件先后与迟到判断不依赖时间窗口：状态机按
  `session_id` 隔离任务，每个会话保留当前轮次及最多 256 个已终止或被明确新
  起点替换的 `turn_id`，据此拒绝旧轮次事件。活跃轮次期间抢先到达的不同 ID
  进度只进入有界隔离区：普通进度不能据此取代当前轮次，明确 WorkStart 可立即
  接管；当前轮次终止后隔离随之清除，同一候选再次到达时可按 Goal 续跑规则建立
  隐式新轮次。状态机再聚合为工具的唯一展示状态，因此一个任务的
  Stop/退出不会覆盖另一个仍在工作的任务。时间只用于
  有界回收：调用方注入单调时间，连续 30 分钟没有事件的会话或终止 tombstone
  会被批量清理；超时只回收内部记录，不释放设备槽位，最后的非空闲状态会回落
  到空闲展示，而已处于空闲的面板内容保持不变。每个工具同时最多跟踪 256 个会话，
  洪峰超过上限时优先淘汰最旧 tombstone 和非活跃会话。`SessionStart` 建立空闲展示，
  `UserPromptSubmit`
  等真实工作起点进入运行，`Stop`（包括用户中断后的轮次停止）回到空闲；
  `SessionEnd` 清空会话内容并留下有时限、保留轮次历史的终止 tombstone。支持
  resume 的协议可由显式 `SessionStart` 覆盖墓碑；Cursor 会拒绝迟到的同 ID
  `SessionStart`，但带未退休 generation 的明确 WorkStart 可建立新 epoch。同一
  会话已有活跃轮次时，携带不同轮次 ID 的迟到 End 只退休 incoming ID，不结束
  当前工作；无轮次 ID 的 End 若在明确 WorkStart 后 250ms 内反向到达也会忽略，
  超过该交接期则仍按真实会话结束处理。重算剩余会话后仅在确实没有其他任务时
  释放展示位。Cursor 的
  `workspaceOpen` 只在尚无真实会话或墓碑时建立默认空闲占位，任一被接纳的真实
  会话事件都会移除该占位。Stop/End 后的 `PostToolUse`、`SubagentStop`、
  `PostCompact` 不会重新激活状态，直到出现新的明确工作起点。桌面端晚于 AI
  会话启动时，真实工作起点、进度、询问、异常或停止事件可以建立隐式会话并立即
  更新展示；单独到达的完成类事件没有足够信息证明仍在运行，直接忽略且不留记录。
  Codex Goal 模式暂停、恢复或自动续跑时可能在同一会话内切换 `turn_id`，且不再
  产生 `UserPromptSubmit`；已停止会话收到不同轮次的进度、询问或异常事件时将其
  视为新轮次的隐式工作起点，而同一已停止轮次的迟到事件继续抑制。
  只有 Codex、Claude Code、Cursor、OpenCode、Qwen Code、Kimi Code、Qoder、Gemini
  CLI、GitHub Copilot CLI 执行重复状态消除、旧轮次判断、结束墓碑和多会话聚合抑制；
  其他 AI 工具统一采用逐事件直通策略，每个受支持 Hook 均按协议映射立即转发，
  不进行抑制。
- Codex、Claude Code、Cursor、OpenCode、Qwen Code、Kimi Code、Qoder、Gemini CLI、
  GitHub Copilot CLI、WorkBuddy、Hermes、OpenClaw、CodeBuddy
  的协议实现分别位于 `domain/monitor/hooks/`
  下的独立文件，并统一实现 `HookProtocol`。Trait 负责约束工具元数据、事件语义、
  原生 handler/config 结构、stdout 约定和托管条目清理；公共生成、合并和状态机
  不得按工具硬编码事件字符串。Cursor 的 `conversation_id`/`generation_id` 在轻量
  relay 边界归一化为 session/turn 上下文；GitHub Copilot CLI 的 `sessionId` 归一化
  为 session 上下文。Cursor 的 `postToolUseFailure` 是当前 generation 内可恢复的
  Error，只有 `stop.status=error` 会以 Error 终止 generation。Cursor 当前的
  `subagentStart`/`subagentStop` 不提供可关联的父 generation：状态机只允许前者在
  尚无已知父轮次时作为无作用域的冷启动工作信号，后者（包括 error）不改写父轮次
  ID、活跃性或展示优先级。
  OpenCode 使用官方自动发现的全局插件文件订阅公开事件流，独立文件只允许覆盖
  带 AIMonitor 标识的内容；WorkBuddy 使用其内置 CodeBuddy Agent 引擎自 v2.48
  起独立的 `~/.workbuddy/settings.json`，不与 CodeBuddy CLI 配置混用；其 command
  Hook 在所有平台均由内置 Git Bash/POSIX shell 执行，写入后需重启 WorkBuddy
  或新建会话以重新加载配置。
  Hermes 以 `~/.hermes/plugins/aimonitor/` 原生插件订阅官方 observer hooks，
  使用会话、轮次、工具、审批与 API 异常事件计算状态；插件需由用户执行
  `hermes plugins enable aimonitor` 明确信任启用。CodeBuddy 使用
  `CODEBUDDY_CONFIG_DIR`（默认 `~/.codebuddy/settings.json`）及其 Git Bash/POSIX
  command Hook 约定。OpenClaw 作为全局原生插件安装到状态目录的
  `extensions/aimonitor/`，主入口、manifest 与 package metadata 在写入前统一校验，
  任一既有文件不是 AIMonitor 管理时整组拒绝覆盖；安装后由用户显式启用插件、
  授予 conversation lifecycle Hook 权限并重启 Gateway。
- Qwen Code 约定 Claude-style 生命周期与权限事件；写入后需重启或新建会话重载配置。
  Kimi Code 在 `($KIMI_CODE_HOME 或 ~/.kimi-code)/config.toml` 使用 `[[hooks]]` TOML
  数组；AIMonitor 只替换带完整 begin/end 标识的托管区块，保留模型、权限及用户规则。
  Windows 由 Kimi Code 的 Git Bash 执行 POSIX relay 命令，写入后需新建会话。
  Qoder 使用 IDE、JetBrains 插件与 CLI 都支持的 `UserPromptSubmit`、`PreToolUse`、
  `PostToolUse`、`PostToolUseFailure` 和 `Stop` 兼容基线，配置改动立即生效。
  Gemini CLI 使用
  `Before/After Agent`、`Before/After Model`、
  `Before/After Tool`、`Session`、`Notification`、`PreCompress`；其中继要求 stdout
  能解析为标准 JSON。GitHub Copilot CLI 使用版本 1 的扁平化 command hooks，配置文件为
  `($COPILOT_HOME 或 ~/.copilot)/hooks/aimonitor.json`，在执行失败时保持 fail-open。
- listener、状态机 worker 与设备投递 workers 分为两个阶段：最小 Hook 事件先进入
  容量 256 的有界队列并按接收顺序推进状态机，不会因设备网络慢而停止计算当前
  状态；listener 只用非阻塞入队，队列已满或 worker 已停止时立即返回 503 并回滚
  待处理计数，不在 Tokio runtime 上等待容量。投递层按“设备 ID + AI 工具”拆成
  独立 worker 和目标队列。尚未发送的
  中间态可按工具策略被新状态覆盖并计入时序抑制，因此单台设备不会形成网络请求
  长队，离线或慢设备也不会阻塞其他在线设备及时收到询问、异常、停止等短暂状态。
  每个目标设备每次转换只转发一次，失败后不重试。Codex、Claude Code、Cursor、
  OpenCode、Qwen Code、Kimi Code、Qoder、Gemini CLI、GitHub Copilot CLI 在每个目标
  队列内使用 latest-wins；其他工具在每个目标队列内使用逐事件 FIFO，不覆盖任何
  尚未发送的事件。Cursor 的 destructive Release 在目标 worker 内额外等待 250ms
  的交接缓冲；同一设备、同一工具在此期间出现新的展示状态时，尚未发送的 Release
  被 latest-wins 抑制，从而覆盖独立 Hook 进程造成的 End/Start 短暂乱序。同一
  250ms 也供状态机消歧刚刚明确开启轮次后反向到达的无 ID End；已经开始发送的
  请求仍不可撤回。
- Rust 在启动后立即执行一次在线设备发现，之后按设置页以“分钟”为单位配置的
  间隔（默认一分钟）持续刷新在线设备快照；后台循环以 1 秒粒度醒来并重新读取当前
  间隔，因此在设置页修改间隔后无需重启即可立即生效。设备发现同时执行
  mDNS 和 UDP 广播两路，并按设备 id 合并结果，而不是任一路先发现到设备
  就跳过另一路——避免只靠 UDP 广播现身的设备（例如 mDNS 多播在某些网络
  下不可达）从列表中消失。两路发现并行执行，mDNS 等待窗口为 8 秒；设备需
  连续两轮都未被发现才会从快照移除，过滤单轮网络抖动。Hook 转发遍历已配置
  路由前，先按快照过滤掉不在线设备，并使用发现得到的最新地址。普通 IPv6
  地址可直接探测；reqwest 0.12 无法表示链路本地 IPv6 所需的 zone identifier，
  因此当前发现与持久化边界会跳过/拒绝所有链路本地 IPv6 候选，而不是保存后失败。
  快照或自动选择
  发生变化时，通过 Tauri event 将带单调 `revision` 的完整
  `MonitorDeviceSnapshot` 写入前端的 TanStack Query 缓存；后台事件与 command
  结果乱序到达时，旧 revision 不得覆盖新状态。前端“重新扫描”/“强制重新检查”仍
  调用原有 command，不依赖定时器。当前选中设备从稳定在线快照中消失时，
  Rust 自动选择排序后的第一台在线设备并持久化为当前设备；没有在线设备时
  保留最后选择，等待后续发现结果。
- 应用配置使用临时文件替换写入；每次载入时校验当前设备、设备路由和
  “设备 ID + AI 工具”Profile 的唯一关联，拒绝重复或悬空数据。设置与
  Profile 更新先持久化成功再替换内存快照；Hooks 配置写入单独串行化，避免
  并发合并覆盖。
- 开机自启由 Rust application 层通过 Tauri autostart 插件管理；自启参数
  `--silent` 只控制窗口可见性，不改变 Hook listener 和转发服务的启动。
- 桌面生命周期由 Rust 管理：Windows/macOS 使用系统托盘，关闭主窗口只隐藏
  应用；托盘首项只负责显示并聚焦窗口，另提供开机自启勾选和真正退出。macOS 使用
  `Accessory` activation policy 隐藏 Dock 图标。
