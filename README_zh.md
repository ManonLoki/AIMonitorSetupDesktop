# AIMonitor Setup

[English](README.md) | **简体中文**

AIMonitor 桌面配置与中继应用。它负责发现局域网内的 AIMonitor 设备，管理
AI 客户端的展示位置、状态图片与 Hooks，并在本机接收 Codex、Claude Code、
Cursor、OpenCode、WorkBuddy、Hermes、OpenClaw 和 CodeBuddy 的事件后转发到
已配置设备。

项目采用 Tauri + React：Rust 是唯一业务后端，React 只负责界面、交互和调用
类型化 Tauri command。

## 功能

- 通过 mDNS 与 UDP 广播自动发现局域网设备，并持续维护在线状态。
- 为每台设备、每个 AI 客户端分别配置 25 个显示位置以及空闲、运行中、询问、
  异常四种展示状态。
- 浏览、筛选、批量上传和管理 JPEG、PNG、GIF、BMP 与 WebP 图片；上传前由
  Rust 后端完成校验、缩放和兼容格式转换。
- 为支持的 AI 客户端写入本机 Hooks 中继配置；命令型 Hook 由 AIMonitor 自身的
  轻量模式归约为四字段事件信封，Windows 不依赖 PowerShell。
- 查看在线设备、Hook listener、转发结果与时序抑制等运行指标。
- 支持中英文界面、开机静默自启、系统托盘、多设备切换与首次使用引导。

## 实机界面

以下截图来自 macOS 上运行的 AIMonitor v2.2.3，与当前仓库版本一致。

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

## 发布构建（维护者手册）

发布入口与 AIMonitorDesktop 使用同一套命令、平台标签和产物命名规则。
macOS 包只有在 Developer ID 签名、公证、票据装订和 Gatekeeper 校验全部通过后，
才会进入 `publish/`。

### 首次配置构建机

安装依赖、Rust 目标和 Windows 交叉构建工具：

```bash
pnpm install
rustup target add aarch64-apple-darwin x86_64-apple-darwin
rustup target add x86_64-pc-windows-msvc
brew install llvm nsis
cargo install --locked cargo-xwin
```

macOS 钥匙串中必须安装有效的 `Developer ID Application` 证书及其私钥：

```bash
security find-identity -v -p codesigning
```

创建 Developer 权限的 App Store Connect API Key，将下载的 `.p8` 私钥保存到
本机安全目录，再把公证凭据写入钥匙串。尖括号内容必须替换为自己的值：

```bash
mkdir -p "$HOME/.appstoreconnect/private_keys"
chmod 700 "$HOME/.appstoreconnect/private_keys"
chmod 600 "$HOME/.appstoreconnect/private_keys/AuthKey_<KEY_ID>.p8"

xcrun notarytool store-credentials AIMonitorNotary \
  --key "$HOME/.appstoreconnect/private_keys/AuthKey_<KEY_ID>.p8" \
  --key-id "<KEY_ID>" \
  --issuer "<ISSUER_ID>"
```

验证钥匙串凭据：

```bash
xcrun notarytool history --keychain-profile AIMonitorNotary
```

证书、证书私钥、API Key、`.p8` 文件和 Issuer ID 都不得提交到仓库。若使用其他
profile 名称，构建前设置 `AIMONITOR_NOTARY_PROFILE`。

### 每次发布

1. 同步修改 `package.json`、`src-tauri/Cargo.toml` 和
   `src-tauri/tauri.conf.json` 中的版本号，三处必须一致。
2. 执行发布前检查：

   ```bash
   pnpm build
   pnpm check
   ```

3. 根据目标选择一个发布命令：

   ```bash
   # macOS 通用架构（Apple Silicon + Intel）
   pnpm run build:mac

   # Windows x64（在 macOS/Linux 上使用 cargo-xwin）
   pnpm run build:win

   # 依次构建 macOS 通用架构和 Windows x64
   pnpm run build:release
   ```

   如只需单一 macOS 架构，可覆盖默认目标：

   ```bash
   AIMONITOR_MAC_TARGET=aarch64-apple-darwin pnpm run build:mac
   AIMONITOR_MAC_TARGET=x86_64-apple-darwin pnpm run build:mac
   ```

4. 命令成功后检查 `publish/`：

   - `AIMonitorSetup-macOS-<架构>-v<版本>.dmg`
   - `AIMonitorSetup-Windows-x64-v<版本>-setup.exe`
   - `AIMonitorSetup-SHA256SUMS.txt`

脚本只会在本次请求的所有平台均构建成功后清空并重建 `publish/`，不会发布
半成品。macOS 自动流程为：Tauri 构建并签名 → 校验 DMG 签名 → 提交 Apple
公证并等待 `Accepted` → staple 公证票据 → Gatekeeper 校验 → 复制安装器。

Windows x64 安装器通过 `cargo-xwin` 和 NSIS 构建，目前使用 `--no-sign`，没有
Authenticode 签名；它与 macOS Developer ID 签名、公证是两套独立机制。

### 发布后验证

将文件名中的版本替换为本次实际版本：

```bash
xcrun stapler validate "publish/AIMonitorSetup-macOS-<架构>-v<版本>.dmg"
spctl --assess --verbose=2 --type open \
  --context context:primary-signature \
  "publish/AIMonitorSetup-macOS-<架构>-v<版本>.dmg"
shasum -a 256 -c publish/AIMonitorSetup-SHA256SUMS.txt
```

`stapler validate` 应成功；`spctl` 输出应包含 `accepted` 和
`source=Notarized Developer ID`。最后建议在另一台 Mac 和一台 Windows 机器上
分别完成安装与首次启动测试。

### 更换电脑或轮换密钥

新电脑需要同时迁移 Developer ID 证书及其私钥，以及 App Store Connect `.p8`
私钥。导入签名证书后，在新电脑重新运行 `notarytool store-credentials`。确认新
配置可以构建和公证后，再在 App Store Connect 撤销不再使用的旧 API Key。

### 常见问题

- 找不到签名身份：确认钥匙串内同时存在证书和对应私钥，再运行
  `security find-identity -v -p codesigning`。
- 找不到 `AIMonitorNotary`：重新执行 `notarytool store-credentials`，或设置正确的
  `AIMONITOR_NOTARY_PROFILE`。
- 公证返回 `Invalid`：从构建输出取得 Submission ID，然后执行
  `xcrun notarytool log <SUBMISSION_ID> --keychain-profile AIMonitorNotary` 查看原因。
- Windows 构建缺少工具：确认 `cargo-xwin`、`makensis` 和 LLVM 已安装，并确保
  `llvm-rc` 在 `PATH` 中。
- DMG 被 Gatekeeper 拦截：不要通过“仍要打开”绕过后直接发布；确认
  `stapler validate` 成功且 `spctl` 显示 `Notarized Developer ID`。

## 项目约束

- [技术栈与版本](docs/technology-stack.md)
- [架构与代码边界](docs/architecture.md)
- [Hooks 事实标准](docs/hooks-contract.md)
- [代理协作规则](AGENTS.md)

## 许可证

本项目源码以 [PolyForm Noncommercial License 1.0.0](LICENSE) 提供，可用于
个人、研究、教育及其他许可证允许的非商业用途，也可在这些用途范围内修改和
分发。未经版权所有者另行书面授权，不得用于商业用途。

由于包含非商业用途限制，本项目属于“源码可用（source-available）”，不属于
OSI 定义的开源软件。
