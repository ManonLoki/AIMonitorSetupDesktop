# 技术栈与版本

最后核对：2026-07-27。JavaScript 版本来自 npm registry，Rust crate
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
| 原生通信 | `@tauri-apps/api` | 2.11.1 | 类型化 `invoke` 调用 |
| 原生目录选择 | `@tauri-apps/plugin-dialog` | 2.7.2 | 仅用于选择 Hooks 配置目录，路径保存与校验仍由 Rust 完成 |

## Rust 后端库

| 用途 | 选择 | 当前锁定版本 | 使用边界 |
| --- | --- | --- | --- |
| 远端 HTTP | reqwest | 0.12.28 | 仅由 Rust application 层访问 AiMonitor 设备接口 |
| 图片解码/缩放/编码 | image（启用 `bmp`、`gif`、`jpeg`、`png`、`webp` feature） | 0.25.10 | 仅由 Rust domain 层在图片上传前使用：JPEG/PNG 长边超过 800px 时等比缩小并重新编码；GIF 校验后原样透传；BMP 与静态 WebP 转 PNG，动画 WebP 保留帧和延时转 GIF，避免依赖展示屏的 WebP 支持 |
| 局域网服务发现 | mdns-sd | 0.20.2 | 仅由 Rust application 层发现 `_aimonitor._tcp.local.` 设备 |
| 网卡与广播地址 | if-addrs | 0.15.0 | 仅由 Rust application 层枚举可用 IPv4 网卡并发送 UDP 定向广播 |
| JSON 持久化 | serde_json | 1.0.151 | 保存设备设置与 AI 实例配置 |
| 原生目录选择 | tauri-plugin-dialog | 2.7.2 | 为前端提供系统目录选择器 |
| 开机自启 | tauri-plugin-autostart | 2.5.1 | 由 Rust 管理桌面端自启，自启参数固定为 `--silent` |
| 单实例 | tauri-plugin-single-instance | 2.4.3 | 静默运行时再次打开应用，唤起已有主窗口 |
| 系统托盘 | Tauri `tray-icon` feature | 2.11.5 | Windows/macOS 托盘菜单、窗口显隐与退出 |

Cargo 二进制目标名固定为 `AIMonitor`，与 Tauri 的 `productName` 和
`mainBinaryName` 保持一致。这样开发构建与正式 `.app` 注册开机自启时，
macOS 都显示 `AIMonitor`，不会退回 Cargo 包名 `ai-monitor-setup`。

本机 Hook 接口使用 Rust 标准库 `TcpListener`，仅绑定
`127.0.0.1:10240`，协议面只覆盖短连接 POST JSON，不引入通用 Web
框架或额外 runner 脚本。命令型 Hook 复用同一个 `AIMonitor` 二进制的
`--aimonitor-hook-relay` 轻量模式，把 AI 原生 stdin 归约为四字段小信封；Windows
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
