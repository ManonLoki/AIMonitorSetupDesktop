// 引入本 feature 的 AI 工具类型定义
import type { AiTool } from "../api/monitor";

// Record 让 AiTool union 新增成员时产生编译错误，避免 Rust 已支持但 UI 静默漏项。
// 对象的声明顺序同时是设置页、监控管理与 Hooks 管理的固定渲染顺序。
const AI_TOOL_LABELS = {
  codex: "Codex",
  claudeCode: "Claude Code",
  cursor: "Cursor",
  openCode: "OpenCode",
  workBuddy: "WorkBuddy",
  hermes: "Hermes",
  openClaw: "OpenClaw",
  codeBuddy: "CodeBuddy",
  qwenCode: "Qwen Code",
  kimiCode: "Kimi Code",
  qoder: "Qoder",
  geminiCli: "Gemini CLI",
  gitHubCopilot: "GitHub Copilot CLI",
} satisfies Record<AiTool, string>;

export const AI_TOOLS: ReadonlyArray<{ value: AiTool; label: string }> =
  Object.entries(AI_TOOL_LABELS).map(([value, label]) => ({
    value: value as AiTool,
    label,
  }));

// 按固定顺序过滤出设置页已勾选的工具，供各页面渲染可见的 Tabs
export function enabledAiTools(enabled: readonly AiTool[]) {
  return AI_TOOLS.filter((tool) => enabled.includes(tool.value));
}
