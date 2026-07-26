import { Alert, Loader, Stack, Text } from "@mantine/core";
import { DeviceConnectPanel } from "./DeviceConnectPanel";

// 组件所需的设备状态入参：加载中标志、是否已配置设备、是否有可用设备，以及用于提示文案的功能名称
interface MonitorDeviceGateProps {
  isPending: boolean;
  hasConfiguredDevice: boolean;
  hasAvailableDevice: boolean;
  featureLabel: string;
}

// 设备门禁：当设备状态还在加载、或没有可用/已配置设备时，渲染提示与连接面板，
// 否则返回 null 交由调用方渲染真正的功能内容
export function monitorDeviceGate({
  isPending,
  hasConfiguredDevice,
  hasAvailableDevice,
  featureLabel,
}: MonitorDeviceGateProps) {
  // 设备状态查询尚未完成时，展示加载态占位
  if (isPending) {
    return (
      <Stack align="center" py="xl">
        <Loader size="sm" />
        <Text size="sm" c="dimmed">
          正在检查 AIMonitor 设备…
        </Text>
      </Stack>
    );
  }

  // 没有可用设备或尚未配置设备时，提示用户并展示设备连接面板供其操作
  if (!hasAvailableDevice || !hasConfiguredDevice) {
    return (
      <Stack gap="lg" maw={860}>
        <Alert color="yellow" title="无可用设备">
          当前未发现在线的 AIMonitor 设备，{featureLabel}
          暂不可用。请确认设备已开机并接入同一局域网。
        </Alert>
        <DeviceConnectPanel />
      </Stack>
    );
  }

  // 设备已就绪，不需要拦截，交回调用方继续渲染实际功能界面
  return null;
}
