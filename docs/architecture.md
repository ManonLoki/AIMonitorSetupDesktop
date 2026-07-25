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
Codex / Claude Code / Cursor Hook
  ↓ POST 127.0.0.1:10240/api/hooks/{tool}，请求体仅含 type
Rust 本机 Hook listener
  ↓ 根据 tool + Hook type 计算 Idle / Running / Asking / Error
Rust 中读取共享用户名，并按 AI 工具遍历已配置设备的 Profile、路由和显示位置
  ↓ 对每台已配置设备 POST / DELETE /api/slots/{slot}
AIMonitor APK（单台失败不终止后续设备）
```

- listener 只绑定环回地址，不向局域网暴露 Hook 接口。
- Hooks 配置直接请求固定的本机地址，不生成 `.sh` 或 `.ps1` runner，也不保存
  监控屏 IP、用户名、图片或状态规则。
- 所有新托管 Hook 命令统一携带大小写固定的 `AIMonitor` 前缀（完整标识如
  `AIMonitor|tool=codex`）。合并、覆盖和后续删除只按该前缀识别。
- 初始化时由独立后台线程尽力删除 v1/v2/v3 旧标识条目和遗留 runner 文件。
  清理失败不阻止应用启动，也不会把旧条目升级为新条目；新规则只在用户写入
  对应工具的 Hooks 配置时生成。非 AIMonitor Hook 始终保留。
- Profile 按“设备 ID + AI 工具”隔离保存显示位置及四种显示状态，不保存
  设备名称或地址。设备名称和地址作为独立设备路由保存在桌面端；显示用户名是
  所有设备共享的全局设置。左下角当前设备只决定 AI 管理页正在编辑哪台设备。
  中继收到 Hook 后按 AI 工具遍历所有已保存设备路由及对应 Profile，逐台转发；
  单台失败不阻止后续设备。未配置的新设备从空 Profile 开始，不继承或转移上
  一台设备的配置。Hooks JSON 仍只连接固定本机中继，因此切换设备时无需重写。
- AI 管理页只保存 Profile。“工作台”只展示 Hook listener、队列及逐设备转发
  结果。设置页只管理共享用户名、开机自启、各 AI 工具配置目录和 Hooks 写入；
  写入时 Rust 读取当前编辑设备对应的已保存 Profile，只生成固定本机中继规则。
  配置目录仍由 Rust 校验并持久化。
- listener 与转发 worker 通过内存队列解耦，按接收顺序处理。网络失败最多
  自动重试三次；终态到达后的两秒内会抑制迟到的完成类事件，避免
  `Stop → SubagentStop` 把屏幕错误地从空闲切回运行中。
- Rust 在启动后立即执行一次在线设备发现，之后每五分钟刷新在线设备快照。
  Hook 转发遍历已配置路由前，先按快照把在线设备排到前面，并优先使用发现
  得到的最新地址；快照发生变化时通过 Tauri event 更新前端的 TanStack Query
  设备缓存。前端“重新扫描”仍调用原有 command，不依赖定时器。
- 开机自启由 Rust application 层通过 Tauri autostart 插件管理；自启参数
  `--silent` 只控制窗口可见性，不改变 Hook listener 和转发服务的启动。
- 桌面生命周期由 Rust 管理：Windows/macOS 使用系统托盘，关闭主窗口只隐藏
  应用；托盘负责显示/隐藏窗口、开机自启勾选和真正退出。macOS 使用
  `Accessory` activation policy 隐藏 Dock 图标。
