# AGENTS.md

本文件适用于整个仓库。开始修改前必须阅读并遵守：

- [`docs/architecture.md`](docs/architecture.md)：权威的分层、状态归属和
  command 合约。
- [`docs/technology-stack.md`](docs/technology-stack.md)：权威的技术选型、
  版本与依赖维护策略。

## 不可破坏的约束

1. 所有业务逻辑在 Rust 后端实现；React 只保留展示、交互与调用薄层。
2. UI 使用 Mantine；路由使用 TanStack Router。
3. Rust command 的异步状态使用 TanStack Query。
4. Jotai 仅用于客户端 UI 状态，禁止保存后端业务数据或 Query 结果。
5. 前端默认只通过类型化 Tauri `invoke` 调用 Rust，不添加 Axios、`fetch`
   或其他浏览器 HTTP 客户端。远端网络访问应由 Rust 实现。
6. React 组件中不得直接裸调用 `invoke`；调用必须经过 feature 的 `api/`
   和 `queries/` 层。
7. Command 只做传输适配；业务判断放入 Rust `domain/` 或后续
   `application/` 层。
8. 不添加未使用依赖。依赖变更必须同步锁文件与
   `docs/technology-stack.md`。

## 完成标准

修改应在适用范围内通过：

```bash
pnpm build
pnpm check
pnpm tauri build
```

新增业务必须优先提供 Rust 单元测试。任何有意偏离上述架构的方案，都必须
先更新架构文档并说明原因，不能只在代码中形成例外。
