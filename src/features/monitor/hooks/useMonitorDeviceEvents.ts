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
