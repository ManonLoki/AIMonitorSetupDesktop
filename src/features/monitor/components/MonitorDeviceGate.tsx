import { Alert, Loader, Stack, Text } from "@mantine/core";
import { DeviceConnectPanel } from "./DeviceConnectPanel";

interface MonitorDeviceGateProps {
  isPending: boolean;
  hasConfiguredDevice: boolean;
  hasAvailableDevice: boolean;
  featureLabel: string;
}

export function monitorDeviceGate({
  isPending,
  hasConfiguredDevice,
  hasAvailableDevice,
  featureLabel,
}: MonitorDeviceGateProps) {
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

  return null;
}
