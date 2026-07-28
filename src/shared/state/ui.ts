// 引入 Jotai 提供的“带持久化存储”的 atom 创建函数（默认基于 localStorage）
import { atom } from "jotai";
import { atomWithStorage } from "jotai/utils";

// Jotai is reserved for client-only UI state, never server or domain state.
// 颜色主题（浅色/深色）状态，持久化到 localStorage 键 "ai-monitor-color-scheme"，默认浅色
export const colorSchemeAtom = atomWithStorage<"light" | "dark">(
  "ai-monitor-color-scheme",
  "light",
);

// 界面语言属于本机 UI 偏好；默认跟随系统，也允许用户显式固定中英文。
export type LanguagePreference = "system" | "zh-CN" | "en";
export const languagePreferenceAtom = atomWithStorage<LanguagePreference>(
  "ai-monitor-language",
  "system",
  undefined,
  { getOnInit: true },
);

// 侧边栏是否折叠的状态，持久化到 localStorage 键 "ai-monitor-sidebar-collapsed"，默认展开
export const sidebarCollapsedAtom = atomWithStorage<boolean>(
  "ai-monitor-sidebar-collapsed",
  false,
);

// 新手引导是否已经完成/跳过。它只描述本机 UI 是否还需要提示，不承载后端业务事实。
export const onboardingCompletedAtom = atomWithStorage<boolean>(
  "ai-monitor-onboarding-completed-v1",
  false,
  undefined,
  { getOnInit: true },
);

// 当前是否展开新手引导；不持久化，重新启动应用后由完成状态决定是否自动打开。
export const onboardingOpenAtom = atom(false);
