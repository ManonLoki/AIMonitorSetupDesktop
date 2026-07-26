import {
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { queryClient } from "./query-client";
import {
  monitorDevicesQuery,
  monitorSettingsQuery,
} from "../features/monitor/queries/monitor";
import { AppShellLayout } from "../shared/ui/AppShellLayout";
import { AppBootScreen } from "../shared/ui/AppBootScreen";
import { AiManagementPage } from "../features/monitor/pages/AiManagementPage";
import { ImagesPage } from "../features/monitor/pages/ImagesPage";
import { SettingsPage } from "../features/monitor/pages/SettingsPage";
import { WorkbenchPage } from "../features/monitor/pages/WorkbenchPage";

const rootRoute = createRootRoute({
  component: AppShellLayout,
  pendingComponent: AppBootScreen,
  pendingMs: 0,
  beforeLoad: async () => {
    await Promise.allSettled([
      queryClient.ensureQueryData(monitorSettingsQuery),
      queryClient.ensureQueryData(monitorDevicesQuery),
    ]);
  },
});

const homeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: WorkbenchPage,
});

const aiManagementRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/ai-management",
  component: AiManagementPage,
});

const imagesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/images",
  component: ImagesPage,
});

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  component: SettingsPage,
});

const routeTree = rootRoute.addChildren([
  homeRoute,
  aiManagementRoute,
  imagesRoute,
  settingsRoute,
]);

export const router = createRouter({
  routeTree,
  defaultPreload: "intent",
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
