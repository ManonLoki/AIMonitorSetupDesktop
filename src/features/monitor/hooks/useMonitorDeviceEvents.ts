import { listen } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import type { DiscoveredMonitorDevice } from "../api/monitor";
import { monitorKeys } from "../queries/monitor";

const MONITOR_DEVICES_CHANGED_EVENT = "monitor-devices-changed";

/**
 * 把 Rust 后台设备发现结果写入 TanStack Query。手动“重新扫描”仍使用原有
 * queryFn/refetch 流程，两条路径共享同一个 query key。
 */
export function useMonitorDeviceEvents() {
  const queryClient = useQueryClient();

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void listen<DiscoveredMonitorDevice[]>(
      MONITOR_DEVICES_CHANGED_EVENT,
      (event) => {
        queryClient.setQueryData(monitorKeys.devices(), event.payload);
        // Rust 会在当前设备离线时同步选择并持久化第一台在线设备。
        // 设备事件到达后刷新设置，避免 UI 继续显示旧选择。
        void queryClient.invalidateQueries({ queryKey: monitorKeys.settings() });
      },
    ).then((stopListening) => {
      if (disposed) {
        stopListening();
      } else {
        unlisten = stopListening;
      }
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [queryClient]);
}
