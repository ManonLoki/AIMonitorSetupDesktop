# 技术栈与版本

最后核对：2026-08-06。JavaScript 版本来自 npm registry，Rust crate
版本来自 crates.io；`pnpm-lock.yaml` 和 `src-tauri/Cargo.lock` 是可复现构建的
最终依据。

## 基础运行时

| 层 | 技术 | 当前版本 |
| --- | --- | --- |
| 桌面容器 | Tauri | 2.11.5 |
| 后端语言 | Rust stable / edition 2024 | 1.97.1 |
| UI 运行时 | React / React DOM | 19.2.8 |
| 构建工具 | Vite | 8.1.5 |
| 类型检查 | TypeScript | 7.0.2 |
| 包管理 | pnpm | 10.30.2 |

## 前端库

| 用途 | 选择 | 当前版本 | 使用边界 |
| --- | --- | --- | --- |
| UI | Mantine Core / Hooks | 9.4.2 | 组件、主题、响应式 UI |
| 动效 | ReactBits `AnimatedContent` / GSAP | ReactBits 源码组件（2026-07-26）/ 3.15.0 | ReactBits 按官方源码组件模式放入 `shared/ui/react-bits/`；GSAP 只负责页面与内容入场，必须尊重系统减少动态效果设置 |
| 路由 | TanStack Router | 1.170.18 | 页面、导航、URL 状态 |
| 异步状态 | TanStack Query | 5.101.4 | Rust command 的生命周期、缓存和失效 |
| 客户端状态 | Jotai | 2.20.2 | 主题、面板开关等纯 UI 状态 |
| 图标 | `@tabler/icons-react` | 3.46.0 | `shared/ui/LineIcon` 把图标名映射到 Tabler 组件，调用方仍只传 `name`/`size` |
| 界面国际化 | `react-i18next` / `i18next` | 17.0.11 / 26.3.6 | 中英文资源仍以类型化 `TranslationKey` 字典维护（`shared/i18n/en.ts`、`zh-CN.ts`），`i18next` 负责运行时插值与语言切换；Rust 命令失败时返回 `{ code, params }` 结构化错误，同一份字典负责翻译，Rust 侧不拼接任何面向用户的文案 |
| 原生通信 | `@tauri-apps/api` | 2.11.1 | 类型化 `invoke` 调用 |
| 原生目录选择 | `@tauri-apps/plugin-dialog` | 2.7.2 | 仅用于选择 Hooks 配置目录，路径保存与校验仍由 Rust 完成 |
| 系统外链 | `@tauri-apps/plugin-opener` | 2.5.4 | 仅允许设置页项目地址通过系统默认浏览器打开 |

## Rust 后端库

| 用途 | 选择 | 当前锁定版本 | 使用边界 |
| --- | --- | --- | --- |
| 结构化日志 | tracing / tracing-subscriber / tracing-appender | 0.1.44 / 0.3.23 / 0.2.5 | `application::logging` 在 Tauri `setup` 的第一步初始化：按天滚动写入 `app_data_dir/logs`，debug 构建额外输出到标准错误；`WorkerGuard` 交给 Tauri 状态管理持有以保证退出前刷盘。日志级别由 `RUST_LOG` 环境变量控制，未设置时默认为 `info` |
| 错误处理 | anyhow / thiserror | 1.0.104 / 2.0.19 | 应用/领域层内部错误统一用 thiserror 定义带 `#[source]` 的错误枚举（如 `ConfigIoError`、`RelayValidationError`），仅在跨越 Tauri 命令边界前转换为 `AppError`；不跨越该边界的编排代码（如命令型 Hook relay 子进程 `hook_relay.rs`）使用 `anyhow::Result` 与 `.context(..)` 传播错误链。`AppError`（`domain/error.rs`）保持不变：它是前端 i18n 契约的可序列化 `{ code, params }` 结构，不属于内部错误传播机制 |
| 远端 HTTP | reqwest | 0.12.28 | 仅由 Rust application 层访问 AIMonitor 设备接口；本机/LAN 客户端关闭系统代理、重定向和库内隐式重试，Hook 启动竞态只使用显式固定重试契约 |
| 命令行解析 | Clap | 4.6.4 | 仅用于解析并校验轻量 Hook relay 的内部命令行参数；桌面 GUI 启动参数不进入 Clap |
| 本机 Hook HTTP listener | Axum / Tokio | 0.8.9 / 1.53.1 | Axum 只提供本机 Hook POST 路由与请求体限制；listener 复用 Tauri 的 Tokio runtime，并仅绑定环回地址 |
| Hook ingress 有界通道 | Tokio MPSC | 1.53.1 | Axum handler 使用非阻塞 `try_send` 投递最小信封；状态机 worker 通过异步接收与定时 tick 保持 FIFO 和会话回收，不在 runtime worker 上阻塞等待队列 |
| 图片解码/缩放/编码 | image（启用 `bmp`、`gif`、`jpeg`、`png`、`webp` feature） | 0.25.10 | 仅由 Rust domain 层在图片上传前使用：JPEG/PNG 长边超过 800px 时等比缩小并重新编码；GIF 校验后原样透传；BMP 与静态 WebP 转 PNG，动画 WebP 保留帧和延时转 GIF，避免依赖展示屏的 WebP 支持 |
| 图片 Data URL 编码 | base64 | 0.22.1 | application 层使用标准 RFC 4648 字母表及 `=` padding 编码设备返回的图片字节，不保留手写位运算实现 |
| MIME 解析 | mime | 0.3.17 | application 层解析设备图片响应的 `Content-Type` 及参数，再交由领域格式白名单判定 |
| 局域网服务发现 | mdns-sd | 0.20.2 | 仅由 Rust application 层发现 `_aimonitor._tcp.local.` 设备 |
| 网卡与广播地址 | if-addrs | 0.15.0 | 仅由 Rust application 层枚举可用 IPv4 网卡并发送 UDP 定向广播 |
| HTTP 基地址解析 | http | 1.4.2 | domain/application 层使用 `Uri` 校验设备 origin 与区分 IPv4/IPv6；普通 IPv6 可用，必须依赖 RFC 6874 zone identifier、但 reqwest 0.12 无法传输的链路本地 IPv6 在发现/持久化边界明确拒绝 |
| 原生 Hook 文本解码 | encoding_rs | 0.8.35 | relay 边界按 BOM 严格解码 UTF-8、UTF-16LE/BE；损坏输入不使用替换字符放行 |
| JSON 持久化 | serde_json | 1.0.151 | 保存设备设置与 AI 实例配置 |
| Kimi TOML 生成 | toml | 1.1.3 | 只序列化 AIMonitor 自己生成的托管 Hook 区块；用户区块仍按字节保留，不解析后重写整份配置 |
| 配置原子替换 | tempfile | 3.27.0 | application 层在目标同目录创建随机临时文件并持久化替换，避免固定临时名冲突及 Windows 直接截断写入 |
| 控制端唯一身份 | uuid | 1.24.0 | 首次启动生成并持久化 `clientId`，用于槽位归属与心跳租约 |
| 原生目录选择 | tauri-plugin-dialog | 2.7.2 | 为前端提供系统目录选择器 |
| 系统外链 | tauri-plugin-opener | 2.5.4 | 通过 capability 白名单只开放项目 GitHub 地址 |
| 开机自启 | tauri-plugin-autostart | 2.5.1 | 由 Rust 管理桌面端自启，自启参数固定为 `--silent` |
| 单实例 | tauri-plugin-single-instance | 2.4.3 | 静默运行时再次打开应用，唤起已有主窗口 |
| 系统托盘 | Tauri `tray-icon` feature | 2.11.5 | Windows/macOS 托盘菜单、窗口显隐与退出 |

Cargo 二进制目标名固定为 `AIMonitor`，与 Tauri 的 `productName` 和
`mainBinaryName` 保持一致。这样开发构建与正式 `.app` 注册开机自启时，
macOS 都显示 `AIMonitor`，不会退回 Cargo 包名 `ai-monitor-setup`。
发布文件使用独立且固定的 `AIMonitorSetup` 前缀，不改变应用和 Hook relay 的
`AIMonitor` 运行时身份；macOS、Windows 与校验文件的命名模板和
AIMonitorDesktop 保持一致。

桌面发布工具链不进入应用运行时依赖：macOS 使用系统 `codesign`、Xcode
`notarytool`/`stapler` 与 Gatekeeper；Windows x64 在 macOS 上使用
`cargo-xwin`、LLVM 与 NSIS。Windows 发布按项目策略明确使用 `--no-sign`，不要求
Authenticode 证书、签名密码、Tauri `signCommand` 或 `osslsigncode`；未签名安装器
是预期发布产物，不是降级 fallback。发布脚本仍在复制前用 `llvm-readobj` 验证
应用 EXE 为 x86_64 MSVC PE，并为两个平台生成统一 SHA-256 校验清单。

本机 Hook 接口使用 Axum 路由并通过 Tokio `TcpListener` 提供服务，仅绑定
`127.0.0.1:10240`；listener 复用 Tauri 的 Tokio runtime，协议面只覆盖短连接
POST JSON，不引入额外 runner 脚本。命令型 Hook 复用同一个 `AIMonitor` 二进制的
`--aimonitor-hook-relay` 轻量模式，并由 Clap 严格解析其工具、事件与管理标识参数，
再把 AI 原生 stdin 归约为四字段小信封；Windows
不依赖 PowerShell/curl，因此不受 PowerShell 5.1 代码页和 CLIXML 错误流影响。
APK/Desktop 转发继续使用 `reqwest`；`blocking` feature 同时供轻量 relay 和
后台设备投递线程使用，均不占用 Tauri UI 线程。

## 为什么不使用 Axios

本应用没有浏览器到业务服务的 HTTP 边界。前端与业务后端同处 Tauri
进程模型，默认通道是官方 `invoke`：

```text
TanStack Query → typed TypeScript API → Tauri invoke → Rust command
```

因此 Axios、ky 或原生 `fetch` 都不是更合适的替代，它们会引入额外的
HTTP 服务、端口、CORS、序列化和安全配置。TanStack Query 并不要求 HTTP；
它可以管理任意 Promise，正适合管理 `invoke`。

如果未来需要访问远端服务，网络请求也应由 Rust 后端发起。届时根据实际
协议在 Rust 中选择客户端（HTTP 场景通常评估 `reqwest`），前端仍只调用
Tauri command。不要提前加入未使用的网络依赖。

## 依赖维护

1. JavaScript 依赖使用 `pnpm update --latest` 检查大版本更新。
2. Rust 使用 `cargo update` 更新锁文件，并在修改 manifest 前核对 crate
   的当前稳定版本。
3. 大版本升级必须阅读官方迁移说明，执行 `pnpm build`、`pnpm check` 和
   `pnpm tauri build`。
4. 不保留未使用依赖；需要时再添加。
5. 提交 manifest 时同时提交对应锁文件。
