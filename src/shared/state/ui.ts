import { atomWithStorage } from "jotai/utils";

// Jotai is reserved for client-only UI state, never server or domain state.
export const colorSchemeAtom = atomWithStorage<"light" | "dark">(
  "ai-monitor-color-scheme",
  "light",
);

export const sidebarCollapsedAtom = atomWithStorage<boolean>(
  "ai-monitor-sidebar-collapsed",
  false,
);
