// 应用路由路径的唯一来源：路由树（router.tsx）、侧边导航（AppShellLayout）、
// 引导流程（OnboardingGuide）都从这里读取路径，避免同一路径在多处手写字符串。
export const ROUTES = {
  home: "/",
  aiManagement: "/ai-management",
  images: "/images",
  settings: "/settings",
} as const;
