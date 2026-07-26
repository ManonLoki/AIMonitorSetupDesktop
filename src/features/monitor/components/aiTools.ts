// 引入本 feature 的 AI 工具类型定义
import type { AiTool } from "../api/monitor";

// 全部受支持工具的取值与展示名映射；设置页勾选列表、监控管理与 Hooks 管理的
// Tabs 都基于这份固定顺序渲染，新增工具时只需在此处补一行。
export const AI_TOOLS: ReadonlyArray<{ value: AiTool; label: string }> = [
  { value: "codex", label: "Codex" },
  { value: "claudeCode", label: "Claude Code" },
  { value: "cursor", label: "Cursor" },
  { value: "openCode", label: "OpenCode" },
  { value: "workBuddy", label: "WorkBuddy" },
  { value: "harness", label: "Harness" },
  { value: "openClaw", label: "OpenClaw" },
  { value: "codeBuddy", label: "CodeBuddy" },
];

// 按固定顺序过滤出设置页已勾选的工具，供各页面渲染可见的 Tabs
export function enabledAiTools(enabled: readonly AiTool[]) {
  return AI_TOOLS.filter((tool) => enabled.includes(tool.value));
}
