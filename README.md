# AI Monitor Setup

Tauri 桌面应用基础工程。Rust 是业务后端，React 是调用 Rust command
并展示结果的薄 UI 层。

## 开发

环境要求：

- Node.js 22.12+
- pnpm 10.30+
- Rust stable（项目当前验证版本为 1.97）
- Tauri 对应平台的系统依赖

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
