// 引入 Tauri 事件监听 API
import { listen } from "@tauri-apps/api/event";
// 引入 TanStack Query 的 queryClient hook，用于直接写入缓存
import { useQueryClient } from "@tanstack/react-query";
// 引入 React 的副作用 hook，用于在组件生命周期内订阅/取消订阅事件
import { useEffect } from "react";
// 引入被发现设备的类型定义
import type { DiscoveredMonitorDevice } from "../api/monitor";
// 引入 monitor 相关的查询键定义
import { monitorKeys } from "../queries/monitor";

// Rust 后端在设备发现结果变化时会发出的事件名
const MONITOR_DEVICES_CHANGED_EVENT = "monitor-devices-changed";

/**
 * 把 Rust 后台设备发现结果写入 TanStack Query。手动“重新扫描”仍使用原有
 * queryFn/refetch 流程，两条路径共享同一个 query key。
 */
export function useMonitorDeviceEvents() {
  // 获取 QueryClient 实例，用于在事件回调里直接写缓存/触发失效
  const queryClient = useQueryClient();

  useEffect(() => {
    // 标记组件是否已卸载，防止监听器在异步 resolve 后仍被误设置
    let disposed = false;
    // 保存取消监听的函数，卸载时调用
    let unlisten: (() => void) | undefined;

    // 订阅设备变化事件；事件到达时用最新的设备列表覆盖查询缓存
    void listen<DiscoveredMonitorDevice[]>(
      MONITOR_DEVICES_CHANGED_EVENT,
      (event) => {
        // 直接把事件负载写入设备列表的查询缓存，跳过重新请求
        queryClient.setQueryData(monitorKeys.devices(), event.payload);
        // Rust 会在当前设备离线时同步选择并持久化第一台在线设备。
        // 设备事件到达后刷新设置，避免 UI 继续显示旧选择。
        void queryClient.invalidateQueries({ queryKey: monitorKeys.settings() });
      },
    ).then((stopListening) => {
      if (disposed) {
        // 组件已卸载：立即取消刚建立的监听，避免泄漏
        stopListening();
      } else {
        // 组件仍在挂载中：保存取消函数，供卸载时调用
        unlisten = stopListening;
      }
    });

    // 组件卸载时的清理逻辑：标记已卸载并取消监听（若已建立）
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [queryClient]);
}
