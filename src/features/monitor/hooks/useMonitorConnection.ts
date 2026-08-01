// 引入 TanStack Query 的 useQuery hook
import { useQuery } from "@tanstack/react-query";
// 引入 Rust 原子设备快照查询配置
import { monitorDevicesQuery } from "../queries/monitor";

// 直接暴露 Rust 生成的设备业务快照，不在前端跨 Query 重建连接状态。
export function useMonitorConnection() {
  const snapshot = useQuery(monitorDevicesQuery);

  return {
    snapshot,
    connectedDevice: snapshot.data?.currentDevice,
    otherDevices: snapshot.data?.otherDevices ?? [],
    isPending: snapshot.isPending,
    hasConfiguredDevice: snapshot.data?.hasConfiguredDevice ?? false,
    hasAvailableDevice: snapshot.data?.hasAvailableDevice ?? false,
  };
}
