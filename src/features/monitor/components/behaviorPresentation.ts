import type { HookBehavior } from "../api/monitor";

// Hook 状态颜色只属于界面呈现；状态成员和顺序仍由 Rust 契约提供。
export const behaviorColors = {
  idle: "gray",
  running: "violet",
  asking: "yellow",
  error: "red",
} satisfies Record<HookBehavior, string>;
