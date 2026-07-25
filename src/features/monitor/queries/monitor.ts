import { queryOptions } from "@tanstack/react-query";
import {
  getMonitorSettings,
  getRuntimeOverview,
  discoverMonitorDevices,
  listAiProfiles,
  listHookConfigLocations,
  listRemoteImages,
} from "../api/monitor";

export const monitorKeys = {
  all: ["monitor"] as const,
  settings: () => [...monitorKeys.all, "settings"] as const,
  devices: () => [...monitorKeys.all, "devices"] as const,
  images: () => [...monitorKeys.all, "images"] as const,
  profiles: () => [...monitorKeys.all, "profiles"] as const,
  hookConfigLocations: () =>
    [...monitorKeys.all, "hook-config-locations"] as const,
  runtime: () => [...monitorKeys.all, "runtime"] as const,
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

export const hookConfigLocationsQuery = queryOptions({
  queryKey: monitorKeys.hookConfigLocations(),
  queryFn: listHookConfigLocations,
  staleTime: Number.POSITIVE_INFINITY,
});

export const runtimeOverviewQuery = queryOptions({
  queryKey: monitorKeys.runtime(),
  queryFn: getRuntimeOverview,
  refetchInterval: 3_000,
});
