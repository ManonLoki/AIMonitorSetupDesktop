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

## 项目约束

- [技术栈与版本](docs/technology-stack.md)
- [架构与代码边界](docs/architecture.md)
- [代理协作规则](AGENTS.md)
