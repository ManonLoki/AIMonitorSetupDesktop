import { queryOptions } from "@tanstack/react-query";
import {
  getMonitorSettings,
  discoverMonitorDevices,
  listAiProfiles,
  listLocalHookConfigs,
  listRemoteImages,
  previewHookConfig,
} from "../api/monitor";
import type { AiProfile } from "../api/monitor";

export const monitorKeys = {
  all: ["monitor"] as const,
  settings: () => [...monitorKeys.all, "settings"] as const,
  devices: () => [...monitorKeys.all, "devices"] as const,
  images: () => [...monitorKeys.all, "images"] as const,
  profiles: () => [...monitorKeys.all, "profiles"] as const,
  localHookConfigs: () =>
    [...monitorKeys.all, "local-hook-configs"] as const,
  hookPreview: (profile: AiProfile) =>
    [...monitorKeys.all, "hook-preview", profile] as const,
};

export const monitorSettingsQuery = queryOptions({
  queryKey: monitorKeys.settings(),
  queryFn: getMonitorSettings,
  staleTime: Number.POSITIVE_INFINITY,
});

export const monitorDevicesQuery = queryOptions({
  queryKey: monitorKeys.devices(),
  queryFn: discoverMonitorDevices,
  staleTime: 10_000,
});

export const remoteImagesQuery = queryOptions({
  queryKey: monitorKeys.images(),
  queryFn: listRemoteImages,
});

export const aiProfilesQuery = queryOptions({
  queryKey: monitorKeys.profiles(),
  queryFn: listAiProfiles,
});

export const localHookConfigsQuery = queryOptions({
  queryKey: monitorKeys.localHookConfigs(),
  queryFn: listLocalHookConfigs,
});

export function hookPreviewQuery(profile: AiProfile) {
  return queryOptions({
    queryKey: monitorKeys.hookPreview(profile),
    queryFn: () => previewHookConfig(profile),
    staleTime: Number.POSITIVE_INFINITY,
  });
}
