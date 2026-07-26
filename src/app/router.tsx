// 引入 TanStack Router 的路由构造函数：根路由、子路由、路由器实例创建
import {
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
// 引入全局唯一的 QueryClient 实例，用于路由加载前预取数据
import { queryClient } from "./query-client";
// 引入监控设备列表与监控配置的查询定义，用于路由级预加载
import {
  monitorDevicesQuery,
  monitorSettingsQuery,
} from "../features/monitor/queries/monitor";
// 引入应用外壳布局组件（导航栏 + 内容区），作为根路由的组件
import { AppShellLayout } from "../shared/ui/AppShellLayout";
// 引入应用启动/加载中的占位屏幕组件
import { AppBootScreen } from "../shared/ui/AppBootScreen";
// 引入各业务页面组件
import { AiManagementPage } from "../features/monitor/pages/AiManagementPage";
import { ImagesPage } from "../features/monitor/pages/ImagesPage";
import { SettingsPage } from "../features/monitor/pages/SettingsPage";
import { WorkbenchPage } from "../features/monitor/pages/WorkbenchPage";

// 根路由：所有子路由共享的外层布局，负责在数据加载完成前展示启动屏幕
const rootRoute = createRootRoute({
  component: AppShellLayout, // 加载完成后渲染的外壳组件（内含 Outlet）
  pendingComponent: AppBootScreen, // 加载中展示的占位组件
  pendingMs: 0, // 立即展示 pending 状态，不等待延迟阈值
  beforeLoad: async () => {
    // 路由进入前预取监控配置与设备列表数据，写入 QueryClient 缓存，
    // 使用 allSettled 保证其中一个请求失败也不会阻塞路由加载
    await Promise.allSettled([
      queryClient.ensureQueryData(monitorSettingsQuery),
      queryClient.ensureQueryData(monitorDevicesQuery),
    ]);
  },
});

// 首页路由："/"，渲染工作台页面
const homeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: WorkbenchPage,
});

// AI 管理页路由："/ai-management"
const aiManagementRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/ai-management",
  component: AiManagementPage,
});

// 图片管理页路由："/images"
const imagesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/images",
  component: ImagesPage,
});

// 设置页路由："/settings"
const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  component: SettingsPage,
});

// 将各子路由挂载到根路由下，组装成完整路由树
const routeTree = rootRoute.addChildren([
  homeRoute,
  aiManagementRoute,
  imagesRoute,
  settingsRoute,
]);

// 创建路由器实例：使用上面的路由树，并在鼠标悬停/触碰等“意图”出现时预加载
export const router = createRouter({
  routeTree,
  defaultPreload: "intent",
});

// 向 TanStack Router 的类型系统注册本应用的路由器类型，
// 使 <Link to="..."> 等 API 能获得基于路由树的类型安全提示
declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
