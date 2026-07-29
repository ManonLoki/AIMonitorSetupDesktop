import type { useI18n } from "../../../shared/i18n";
import type { AiTool } from "../api/monitor";

type Translate = ReturnType<typeof useI18n>["t"];

// 写入配置后的工具内操作提示。单独维护以免 HooksManagementCard 随工具数量
// 持续膨胀，并与 Rust HookProtocol 的 review/restart 声明保持一一对应。
export function hookActivationGuidance(
  tool: AiTool,
  toolLabel: string,
  filename: string,
  configChanged: boolean,
  t: Translate,
): string | null {
  if (!configChanged) {
    return t("hooks.guidanceUnchanged");
  }
  switch (tool) {
    case "codex":
      return t("hooks.guidanceCodex", { filename });
    case "workBuddy":
      return t("hooks.guidanceWorkBuddy", { filename });
    case "codeBuddy":
      return t("hooks.guidanceCodeBuddy", { filename });
    case "openClaw":
      return t("hooks.guidanceOpenClaw", { filename });
    case "hermes":
      return t("hooks.guidanceHermes", { filename });
    case "qwenCode":
    case "kimiCode":
    case "geminiCli":
    case "gitHubCopilot":
      return t("hooks.guidanceRestart", { filename, tool: toolLabel });
    default:
      return null;
  }
}
