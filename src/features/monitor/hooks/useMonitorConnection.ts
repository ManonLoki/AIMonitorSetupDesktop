// 引入 TanStack Query 的 useQuery hook
import { useQuery } from "@tanstack/react-query";
// 引入设备列表查询与监控设置查询的配置
import { monitorDevicesQuery, monitorSettingsQuery } from "../queries/monitor";

// 组合监控设置与已发现设备两个查询，派生出“当前已连接设备”“其他可选设备”等状态，供页面直接消费
export function useMonitorConnection() {
  // 当前监控设置（包含已选中的 deviceId）
  const settings = useQuery(monitorSettingsQuery);
  // 局域网内已发现的设备列表
  const devices = useQuery(monitorDevicesQuery);

  // 在设备列表中查找与当前设置里 deviceId 匹配的那台，即当前已连接设备
  const connectedDevice = devices.data?.find(
    (device) => device.id === settings.data?.deviceId,
  );
  // 除已连接设备外的其余可选设备
  const otherDevices = (devices.data ?? []).filter(
    (device) => device.id !== connectedDevice?.id,
  );

  return {
    // 原始设置查询结果，供页面访问加载态/错误态等
    settings,
    // 原始设备列表查询结果
    devices,
    // 当前已连接的设备（可能不存在）
    connectedDevice,
    // 除已连接设备外的其他设备列表
    otherDevices,
    // 两个查询只要有一个仍在首次加载，即视为整体处于 pending 状态
    isPending: settings.isPending || devices.isPending,
    // 是否已经配置过设备（settings 中存在 deviceId）
    hasConfiguredDevice: Boolean(settings.data?.deviceId),
    // 是否存在至少一台可用（已发现）的设备
    hasAvailableDevice: (devices.data?.length ?? 0) > 0,
  };
}
