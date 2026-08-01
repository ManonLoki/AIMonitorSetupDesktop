import type { AiTool, AiToolDescriptor } from "../api/monitor";

export interface AiToolOption {
  value: AiTool;
  label: string;
}

// 只做展示适配与可见性筛选；成员、名称和顺序全部沿用 Rust 返回的目录。
export function enabledAiTools(
  catalog: readonly AiToolDescriptor[],
  enabled: readonly AiTool[],
): AiToolOption[] {
  return catalog
    .filter(({ tool }) => enabled.includes(tool))
    .map(({ tool, name }) => ({ value: tool, label: name }));
}
