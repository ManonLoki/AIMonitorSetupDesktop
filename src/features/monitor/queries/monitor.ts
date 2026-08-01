// 引入 TanStack Query 的 queryOptions 工具函数，用于集中声明查询配置
import { queryOptions } from "@tanstack/react-query";
// 引入本 feature 对应的各个类型化 Tauri 调用函数
import {
  getMonitorSettings,
  getMonitorCapabilities,
  getRuntimeOverview,
  discoverMonitorDevices,
  listAiProfileDrafts,
  listHookConfigLocations,
  listRemoteImages,
} from "../api/monitor";
import type { MonitorDeviceSnapshot } from "../api/monitor";

// 异步命令返回与后台事件可能交错，只允许同版本或更高版本的 Rust 快照进入缓存。
export function preferLatestDeviceSnapshot(
  current: MonitorDeviceSnapshot | undefined,
  incoming: MonitorDeviceSnapshot,
) {
  return current && current.revision > incoming.revision ? current : incoming;
}

// 统一管理 monitor 相关的 TanStack Query 查询键，避免各处硬编码字符串数组
export const monitorKeys = {
  // 所有 monitor 相关查询的公共前缀
  all: ["monitor"] as const,
  // 监控设置的查询键
  settings: () => [...monitorKeys.all, "settings"] as const,
  capabilities: () => [...monitorKeys.all, "capabilities"] as const,
  // 已发现设备列表的查询键
  devices: () => [...monitorKeys.all, "devices"] as const,
  // 远程图片列表的查询键
  images: (deviceId?: string) =>
    deviceId
      ? ([...monitorKeys.all, "images", deviceId] as const)
      : ([...monitorKeys.all, "images"] as const),
  // AI Profile 列表的查询键
  profiles: (deviceId?: string) =>
    deviceId
      ? ([...monitorKeys.all, "profiles", deviceId] as const)
      : ([...monitorKeys.all, "profiles"] as const),
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

// 工具目录和数值范围由 Rust 领域层静态声明，应用运行期间不会变化。
export const monitorCapabilitiesQuery = queryOptions({
  queryKey: monitorKeys.capabilities(),
  queryFn: getMonitorCapabilities,
  staleTime: Number.POSITIVE_INFINITY,
});

// 已发现设备列表查询：10 秒内视为新鲜，避免频繁触发设备发现
export const monitorDevicesQuery = queryOptions({
  queryKey: monitorKeys.devices(),
  queryFn: discoverMonitorDevices,
  structuralSharing: (current, incoming) =>
    preferLatestDeviceSnapshot(
      current as MonitorDeviceSnapshot | undefined,
      incoming as MonitorDeviceSnapshot,
    ),
  staleTime: 10_000,
});

// 远程图片列表查询，使用默认的新鲜度策略
export const remoteImagesQuery = (deviceId: string) =>
  queryOptions({
    queryKey: monitorKeys.images(deviceId),
    queryFn: () => listRemoteImages(deviceId),
  });

// AI Profile 列表查询，使用默认的新鲜度策略
export const aiProfilesQuery = (deviceId: string) =>
  queryOptions({
    queryKey: monitorKeys.profiles(deviceId),
    queryFn: listAiProfileDrafts,
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
