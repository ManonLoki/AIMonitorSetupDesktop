import { useEffect, useState } from "react";
import type { AiTool } from "../api/monitor";

/** 维护当前选中的工具 Tab；已选工具被禁用/隐藏后自动回退到第一个可见工具。 */
export function useActiveVisibleTool(
  visibleTools: ReadonlyArray<{ value: AiTool }>,
) {
  const [activeTool, setActiveTool] = useState<AiTool | null>(null);

  useEffect(() => {
    if (visibleTools.length === 0) {
      setActiveTool(null);
    } else if (!visibleTools.some((tool) => tool.value === activeTool)) {
      setActiveTool(visibleTools[0].value);
    }
  }, [activeTool, visibleTools]);

  return [activeTool, setActiveTool] as const;
}
