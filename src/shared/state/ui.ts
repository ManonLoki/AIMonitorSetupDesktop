// 引入 Jotai 提供的“带持久化存储”的 atom 创建函数（默认基于 localStorage）
import { atomWithStorage } from "jotai/utils";

// Jotai is reserved for client-only UI state, never server or domain state.
// 颜色主题（浅色/深色）状态，持久化到 localStorage 键 "ai-monitor-color-scheme"，默认浅色
export const colorSchemeAtom = atomWithStorage<"light" | "dark">(
  "ai-monitor-color-scheme",
  "light",
);

// 侧边栏是否折叠的状态，持久化到 localStorage 键 "ai-monitor-sidebar-collapsed"，默认展开
export const sidebarCollapsedAtom = atomWithStorage<boolean>(
  "ai-monitor-sidebar-collapsed",
  false,
);
