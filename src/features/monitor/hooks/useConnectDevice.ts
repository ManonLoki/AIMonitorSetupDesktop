// 引入 TanStack Query 的 mutation 与 queryClient 相关 hook
import { useMutation, useQueryClient } from "@tanstack/react-query";
// 引入选择设备的类型化 Tauri 调用函数
import { selectMonitorDevice } from "../api/monitor";
// 引入被发现设备的类型定义
import type {
  DiscoveredMonitorDevice,
  MonitorDeviceSnapshot,
} from "../api/monitor";
// 引入 monitor 相关的查询键定义
import {
  monitorKeys,
  preferLatestDeviceSnapshot,
} from "../queries/monitor";

// 提供“连接到某台已发现设备”的 mutation：负责调用后端选中设备，并同步刷新相关缓存
export function useConnectDevice() {
  // 获取当前的 QueryClient 实例，用于手动读写缓存
  const queryClient = useQueryClient();

  return useMutation({
    // mutation 主体：把选中的设备传给后端命令，完成设备切换
    mutationFn: (device: DiscoveredMonitorDevice) =>
      selectMonitorDevice(device),
    // 发起请求前：取消可能还在进行中的旧设备的 profiles/images 请求，避免结果覆盖新状态
    onMutate: async () => {
      await Promise.all([
        queryClient.cancelQueries({ queryKey: monitorKeys.profiles() }),
        queryClient.cancelQueries({ queryKey: monitorKeys.images() }),
      ]);
    },
    // 请求成功后直接写入 Rust 返回的原子快照，并重置所有设备相关缓存。
    onSuccess: (snapshot) => {
      const previous = queryClient.getQueryData<MonitorDeviceSnapshot>(
        monitorKeys.devices(),
      );
      const latest = preferLatestDeviceSnapshot(previous, snapshot);
      if (latest !== snapshot) return;
      queryClient.setQueryData(monitorKeys.devices(), latest);
      void Promise.all([
        queryClient.invalidateQueries({ queryKey: monitorKeys.settings() }),
        queryClient.resetQueries({ queryKey: monitorKeys.profiles() }),
        queryClient.resetQueries({ queryKey: monitorKeys.images() }),
      ]);
    },
  });
}
