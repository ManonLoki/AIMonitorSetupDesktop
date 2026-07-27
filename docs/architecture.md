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
| 短生命周期 UI 状态 | Jotai / 组件本地状态 | 主题、侧栏开关、未提交输入 |

禁止把 Rust 返回的数据复制进 Jotai。这样会产生双重事实来源，也绕过 Query
的缓存失效模型。

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
Codex / Claude Code / Cursor / WorkBuddy / CodeBuddy command Hook
  ↓ AIMonitor --aimonitor-hook-relay：读取原生 stdin，仅提取必要上下文
OpenCode / Hermes / OpenClaw native plugin
  ↓ 直接构造相同的最小信封
POST 127.0.0.1:10240/api/hooks/{tool}
  ↓ { hook_event_name, session_id?, turn_id?, status? } + 受配置控制的事件头
Rust 本机 Hook listener
  ↓ 根据 tool + Hook type 推进纯 Rust 生命周期状态机
Rust 中读取共享用户名，并按 AI 工具遍历已配置设备的 Profile、路由和显示位置
  ↓ 对每台已配置设备 POST / DELETE /api/slots/{slot}
AIMonitor Android / Desktop（单台失败不终止后续设备）
```

- listener 只绑定环回地址，不向局域网暴露 Hook 接口。
- 命令型 Hooks 不依赖 PowerShell、curl 或额外 `.sh`/`.ps1` 文件，而是调用当前
  已安装 AIMonitor 可执行文件的轻量 relay 子命令；该模式在 `main` 入口先行
  截获，不初始化 Tauri、单实例插件或 UI。配置不保存监控屏 IP、用户名、图片或
  状态规则；应用安装路径移动后需重新执行一次 Hooks 写入。
- AI 客户端与桌面端同时冷启动时，Hook 可能早于本机 listener 完成绑定；轻量
  relay 会对本机中继的连接拒绝额外重试 5 次、每次间隔 1 秒。该重试只覆盖
  工具到本机中继的启动竞态，不改变中继到设备“一次转换只投递一次”的规则。
- 所有新托管 Hook 命令统一携带大小写固定的 `AIMonitor` 前缀（完整标识如
  `AIMonitor:tool=codex`）。标识不包含 shell 元字符，避免 Windows Hook 宿主在
  `cmd.exe` 前额外经过 PowerShell 时把参数拆成管道。合并、覆盖和后续删除只按
  该前缀识别；旧版 `AIMonitor|tool=...` 标识仅用于识别并自动迁移。
- Windows 原生 command Hook 执行器只写入标准 `cmd.exe` 命令；不同时混入
  POSIX `command` 与 `commandWindows`。WorkBuddy、CodeBuddy 是例外：客户端自身
  固定通过随产品提供的 Git Bash/POSIX shell 执行 Hook，因此配置直接启动
  `AIMonitor.exe`，不调用外部脚本、curl 或用户另行安装的 Bash。
- 启动时只迁移配置文件中已存在且带 `AIMonitor` 标识的受管 Hook 条目，用于把
  旧版直接转发原始 stdin 的命令升级为当前轻量 relay；不会为未配置的工具创建
  Hook，也不会修改非 AIMonitor Hook。除此之外不保留旧 Hook 标识、runner 文件
  或旧 Profile 字段/行为的兼容层。
- Profile 按“设备 ID + AI 工具”隔离保存显示位置及四种显示状态，不保存
  设备名称或地址。设备名称和地址作为独立设备路由保存在桌面端；显示用户名是
  所有设备共享的全局设置。左下角当前设备只决定监控管理页正在编辑哪台设备。
  中继收到 Hook 后按 AI 工具遍历所有已保存设备路由及对应 Profile，并发转发给
  每一台（先一次性发起所有设备的转发线程，再统一等待结果，避免某台设备网络
  慢或不可达时拖慢其余设备收到状态更新的时间）；单台失败不阻止其余设备。
  未配置的新设备从空 Profile 开始，不继承或转移上一台设备的配置。Hooks
  JSON 仍只连接固定本机中继，因此切换设备时无需重写。
- 监控管理页只保存所选 AI 客户端的 Profile；设置页持久化 AI 客户端多选范围，
  默认启用 Codex、Claude Code、Cursor，并由该范围共同控制监控管理和 Hooks 管理
  的选项卡。各 AI 工具配置目录与写入 Hooks 配置集中在设置页独立的“Hooks 管理”
  卡片。“工作台”展示 Hook listener、队列及中继统计
  （收到/转发成功/失败/待处理/时序抑制次数，以及最近一次事件与错误——均为跨设备的聚合计数，不含逐设备明细），
  以及当前在线设备列表（复用发现设备的 TanStack Query 缓存），并提供“强制
  重新检查”按钮立即触发一次设备发现，不等待后台定时器。设置页卡片依次为
  “AI 客户端”“Hooks 管理”“通用设置”；通用设置管理共享用户名、
  开机自启、在线设备自动检查间隔及重新触发新手引导。新手引导属于本机持久化
  的纯 UI 状态，首次运行自动显示，完成或跳过后不再自动出现；它依次引导用户
  选择 AI 客户端并保存、写入 Hooks、可选修改用户名、上传图片，最后设置显示
  位置与四种行为并保存 Profile。写入时 Rust 只生成固定本机中继规则，不依赖
  设备 Profile，因此可以先写入 Hooks，再在监控管理中保存展示配置。Hooks
  配置目录仍由 Rust 校验并持久化。用户名缺省时由 Rust 使用本机系统用户名。
  完全没有在线设备时，监控管理和图片管理
  入口禁用，页面也不发起设备业务请求；侧栏明确显示“无可用设备”。
- 命令型 Hook 的轻量 relay 在 HTTP 之前解析工具写入 stdin 的原生 JSON，只保留
  `hook_event_name`、`session_id`、`turn_id`、`status`；同时兼容从原生输入读取
  Cursor 的 `conversation_id`/`generation_id` 与 WorkBuddy 的 camelCase
  `sessionId`/`turnId`，但在线路上统一使用规范字段。
  prompt、transcript、tool input/output 等无关或敏感大字段不会进入 listener。
  listener 正文上限为 4 KiB，并以 `deny_unknown_fields` 严格拒绝契约外字段；
  `X-AIMonitor-Hook-Type` 必须存在且必须与正文事件一致。
  事件到状态的归一化、重复事件消除、迟到完成事件抑制和会话释放全部由
  `domain/monitor/hook_state_machine` 的状态机决定。事件先后与迟到判断不依赖时间窗口：状态机按
  `session_id` 隔离任务、按 `turn_id` 拒绝旧轮次事件，再聚合为工具的唯一展示
  状态，因此一个任务的 Stop/退出不会覆盖另一个仍在工作的任务。时间只用于
  有界回收：调用方注入单调时间，连续 30 分钟没有事件的会话或终止 tombstone
  会被批量清理；超时只回收内部记录，不释放设备槽位，最后的非空闲状态会回落
  到空闲展示，而已处于空闲的面板内容保持不变。每个工具同时最多跟踪 256 个会话，
  洪峰超过上限时优先淘汰最旧 tombstone 和非活跃会话。`SessionStart` 建立空闲展示，
  `UserPromptSubmit`
  等真实工作起点进入运行，`Stop`（包括用户中断后的轮次停止）回到空闲；
  `SessionEnd` 清空会话内容并留下有时限的终止 tombstone，重算剩余会话后仅在
  确实没有其他任务时释放展示位。Stop/End 后的 `PostToolUse`、`SubagentStop`、
  `PostCompact` 不会重新激活状态，直到出现新的明确工作起点。桌面端晚于 AI
  会话启动时，真实工作起点、进度、询问、异常或停止事件可以建立隐式会话并立即
  更新展示；单独到达的完成类事件没有足够信息证明仍在运行，直接忽略且不留记录。
  只有 Codex、Claude Code、Cursor、OpenCode 执行重复状态消除、旧轮次判断、
  结束墓碑和多会话聚合抑制；其他 AI 工具统一采用逐事件直通策略，每个受支持
  Hook 均按协议映射立即转发，不进行抑制。
- Codex、Claude Code、Cursor、OpenCode、WorkBuddy、Hermes、OpenClaw、CodeBuddy
  的协议实现分别位于 `domain/monitor/hooks/`
  下的独立文件，并统一实现 `HookProtocol`。Trait 负责约束工具元数据、事件语义、
  原生 handler/config 结构、stdout 约定和托管条目清理；公共生成、合并和状态机
  不得按工具硬编码事件字符串。Cursor 的 `conversation_id`/`generation_id` 在
  轻量 relay 边界归一化为 session/turn，`stop.status=error` 归一化为异常展示。
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
- listener、状态机 worker 与设备投递 workers 分为两个阶段：最小 Hook 事件先进入
  容量 256 的有界队列并按接收顺序推进状态机，不会因设备网络慢而停止计算当前
  状态；投递层按 AI 工具拆成独立 worker 和 latest-wins mailbox，每个工具
  最多保留一个发送中状态和一个待发送的最新状态。尚未发送的中间态被新状态覆盖
  并计入时序抑制，因此同一工具对应的配置位置不会形成网络请求长队；不同工具
  之间也不会因为某一个慢请求而彼此串行阻塞。每个目标设备每次转换只转发一次，
  失败后不重试。只有 Codex、Claude Code、Cursor、OpenCode 使用 latest-wins
  并执行上述时序抑制；其他工具的专属投递 worker 使用逐事件 FIFO 队列，不覆盖
  任何尚未发送的事件。
- Rust 在启动后立即执行一次在线设备发现，之后按设置页以“分钟”为单位配置的
  间隔（默认一分钟）持续刷新在线设备快照；后台循环以 1 秒粒度醒来并重新读取当前
  间隔，因此在设置页修改间隔后无需重启即可立即生效。设备发现同时执行
  mDNS 和 UDP 广播两路，并按设备 id 合并结果，而不是任一路先发现到设备
  就跳过另一路——避免只靠 UDP 广播现身的设备（例如 mDNS 多播在某些网络
  下不可达）从列表中消失。两路发现并行执行，mDNS 等待窗口为 8 秒；设备需
  连续两轮都未被发现才会从快照移除，过滤单轮网络抖动。Hook 转发遍历已配置
  路由前，先按快照把在线设备
  排到前面，并优先使用发现得到的最新地址；快照发生变化时通过 Tauri event
  更新前端的 TanStack Query 设备缓存。前端“重新扫描”/“强制重新检查”仍
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
