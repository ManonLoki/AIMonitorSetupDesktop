import {
  createRootRoute,
  createRoute,
  createRouter,
  redirect,
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

async function hasConnectedDevice() {
  try {
    const [settings, devices] = await Promise.all([
      queryClient.ensureQueryData(monitorSettingsQuery),
      queryClient.ensureQueryData(monitorDevicesQuery),
    ]);
    return devices.some((device) => device.id === settings.deviceId);
  } catch {
    return false;
  }
}

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
  component: AiManagementPage,
  beforeLoad: async () => {
    if (!(await hasConnectedDevice())) {
      throw redirect({ to: "/settings" });
    }
  },
});

const imagesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/images",
  component: ImagesPage,
  beforeLoad: async () => {
    if (!(await hasConnectedDevice())) {
      throw redirect({ to: "/settings" });
    }
  },
});

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  component: SettingsPage,
});

const routeTree = rootRoute.addChildren([
  homeRoute,
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
