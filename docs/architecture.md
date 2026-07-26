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
- 不包含旧 Hook 标识、runner 文件或旧 Profile 字段/行为的兼容与迁移层；
  当前规则只按 `AIMonitor` 标识幂等更新，非 AIMonitor Hook 始终保留。
- Profile 按“设备 ID + AI 工具”隔离保存显示位置及四种显示状态，不保存
  设备名称或地址。设备名称和地址作为独立设备路由保存在桌面端；显示用户名是
  所有设备共享的全局设置。左下角当前设备只决定监控管理页正在编辑哪台设备。
  中继收到 Hook 后按 AI 工具遍历所有已保存设备路由及对应 Profile，并发转发给
  每一台（先一次性发起所有设备的转发线程，再统一等待结果，避免某台设备网络
  慢或不可达时拖慢其余设备收到状态更新的时间）；单台失败不阻止其余设备。
  未配置的新设备从空 Profile 开始，不继承或转移上一台设备的配置。Hooks
  JSON 仍只连接固定本机中继，因此切换设备时无需重写。
- 监控管理页保存 Profile、各 AI 工具配置目录并写入 Hooks 配置。“工作台”展示 Hook listener、队列及逐设备转发结果，
  以及当前在线设备列表（复用发现设备的 TanStack Query 缓存），并提供“强制
  重新检查”按钮立即触发一次设备发现，不等待后台定时器。设置页管理共享用户名、
  开机自启和在线设备自动检查间隔；写入时
  Rust 读取当前编辑设备对应的已保存 Profile，只生成固定本机中继规则。配置
  配置目录仍由 Rust 校验并持久化。完全没有在线设备时，监控管理和图片管理
  入口禁用，页面也不发起设备业务请求；侧栏明确显示“无可用设备”。
- listener 与转发 worker 通过内存队列解耦，按接收顺序处理。每个目标设备
  只转发一次，失败后不重试，避免积压事件形成请求风暴；终态到达后的两秒内会抑制迟到的完成类事件，避免
  `Stop → SubagentStop` 把屏幕错误地从空闲切回运行中。
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
  应用；托盘负责显示/隐藏窗口、开机自启勾选和真正退出。macOS 使用
  `Accessory` activation policy 隐藏 Dock 图标。
