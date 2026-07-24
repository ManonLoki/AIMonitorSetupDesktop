import {
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { AppShellLayout } from "../shared/ui/AppShellLayout";
import { AiManagementPage } from "../features/monitor/pages/AiManagementPage";
import { ImagesPage } from "../features/monitor/pages/ImagesPage";
import { SettingsPage } from "../features/monitor/pages/SettingsPage";

const rootRoute = createRootRoute({
  component: AppShellLayout,
});

const homeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
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
