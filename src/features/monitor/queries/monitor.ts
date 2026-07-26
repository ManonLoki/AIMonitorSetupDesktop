// 引入 TanStack Query 的 queryOptions 工具函数，用于集中声明查询配置
import { queryOptions } from "@tanstack/react-query";
// 引入本 feature 对应的各个类型化 Tauri 调用函数
import {
  getMonitorSettings,
  getRuntimeOverview,
  discoverMonitorDevices,
  listAiProfiles,
  listHookConfigLocations,
  listRemoteImages,
} from "../api/monitor";

// 统一管理 monitor 相关的 TanStack Query 查询键，避免各处硬编码字符串数组
export const monitorKeys = {
  // 所有 monitor 相关查询的公共前缀
  all: ["monitor"] as const,
  // 监控设置的查询键
  settings: () => [...monitorKeys.all, "settings"] as const,
  // 已发现设备列表的查询键
  devices: () => [...monitorKeys.all, "devices"] as const,
  // 远程图片列表的查询键
  images: () => [...monitorKeys.all, "images"] as const,
  // AI Profile 列表的查询键
  profiles: () => [...monitorKeys.all, "profiles"] as const,
  // hook 配置文件位置列表的查询键
  hookConfigLocations: () =>
    [...monitorKeys.all, "hook-config-locations"] as const,
  // 运行时概览信息的查询键
  runtime: () => [...monitorKeys.all, "runtime"] as const,
};

// 监控设置查询：只在需要时手动失效，默认视为永久新鲜（不会自动重新请求）
export const monitorSettingsQuery = queryOptions({
  queryKey: monitorKeys.settings(),
  queryFn: getMonitorSettings,
  staleTime: Number.POSITIVE_INFINITY,
});

// 已发现设备列表查询：10 秒内视为新鲜，避免频繁触发设备发现
export const monitorDevicesQuery = queryOptions({
  queryKey: monitorKeys.devices(),
  queryFn: discoverMonitorDevices,
  staleTime: 10_000,
});

// 远程图片列表查询，使用默认的新鲜度策略
export const remoteImagesQuery = queryOptions({
  queryKey: monitorKeys.images(),
  queryFn: listRemoteImages,
});

// AI Profile 列表查询，使用默认的新鲜度策略
export const aiProfilesQuery = queryOptions({
  queryKey: monitorKeys.profiles(),
  queryFn: listAiProfiles,
});

// hook 配置文件位置查询：配置目录变化很少，视为永久新鲜
export const hookConfigLocationsQuery = queryOptions({
  queryKey: monitorKeys.hookConfigLocations(),
  queryFn: listHookConfigLocations,
  staleTime: Number.POSITIVE_INFINITY,
});

// 运行时概览查询：每 3 秒自动轮询一次，保持 hook 中继状态等信息实时更新
export const runtimeOverviewQuery = queryOptions({
  queryKey: monitorKeys.runtime(),
  queryFn: getRuntimeOverview,
  refetchInterval: 3_000,
});
