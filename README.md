# AI Monitor Setup

AIMonitor 桌面配置与中继应用。它负责发现局域网内的 AIMonitor 设备，管理
AI 客户端的展示位置、状态图片与 Hooks，并在本机接收 Codex、Claude Code、
Cursor 等工具的事件后转发到已配置设备。

项目采用 Tauri + React：Rust 是唯一业务后端，React 只负责界面、交互和调用
类型化 Tauri command。

## 功能

- 通过 mDNS 与 UDP 广播自动发现局域网设备，并持续维护在线状态。
- 为每台设备、每个 AI 客户端分别配置 25 个显示位置以及空闲、运行中、询问、
  异常四种展示状态。
- 浏览、筛选、批量上传和管理设备图片。
- 为支持的 AI 客户端写入本机 Hooks 中继配置。
- 查看在线设备、Hook listener、转发结果与时序抑制等运行指标。
- 支持开机静默自启、系统托盘、多设备切换与首次使用引导。

## 实机界面

以下截图来自 macOS 上按本文开发流程运行的 AIMonitor v2.0.9。

### 启动与设备扫描

应用启动时会同时通过 mDNS、UDP 广播和已保存地址检查可用设备。

![启动与设备扫描](docs/screenshots/device-scan.jpg)

### 工作台

集中展示在线设备以及本机 Hook 中继的接收、转发、失败、等待处理和时序抑制
状态。

![工作台](docs/screenshots/workbench.jpg)

### 监控管理

按设备与 AI 客户端隔离保存显示位置和四种行为状态的展示配置。

![监控管理](docs/screenshots/monitor-management.jpg)

### 图片管理

查看设备图片数量与格式分类，并支持刷新、筛选和批量上传。

![图片管理](docs/screenshots/image-management.jpg)

### 设置

管理启用的 AI 客户端、Hooks 配置目录、显示用户名、设备检查间隔和开机自启。

![设置](docs/screenshots/settings.jpg)

### 新手引导

首次运行时按顺序引导完成 AI 客户端选择、Hooks 写入、图片上传和展示配置。

![新手引导](docs/screenshots/onboarding.jpg)

## 开发

环境要求：

- Node.js 22.12+
- pnpm 10.30+
- Rust stable（项目当前验证版本为 1.97）
- Tauri 对应平台的系统依赖

正常启动流程：

```bash
pnpm install
pnpm tauri dev
```

常用检查：

```bash
pnpm build
pnpm check
pnpm tauri build
```

## 发布构建

在 macOS 上一次性构建 macOS（ARM64）与 Windows（x64，通过
`cargo-xwin` 交叉编译）安装包：

```bash
pnpm release:desktop
```

产物输出到 `publish/`。运行前需要 `cargo-xwin`、`makensis`（NSIS）等
工具，脚本会在开始前检查依赖是否齐全。

## 项目约束

- [技术栈与版本](docs/technology-stack.md)
- [架构与代码边界](docs/architecture.md)
- [代理协作规则](AGENTS.md)

## 许可证

本项目源码以 [PolyForm Noncommercial License 1.0.0](LICENSE) 提供，可用于
个人、研究、教育及其他许可证允许的非商业用途，也可在这些用途范围内修改和
分发。未经版权所有者另行书面授权，不得用于商业用途。

由于包含非商业用途限制，本项目属于“源码可用（source-available）”，不属于
OSI 定义的开源软件。
