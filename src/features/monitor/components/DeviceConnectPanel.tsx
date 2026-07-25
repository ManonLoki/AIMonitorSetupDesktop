import {
  Alert,
  Badge,
  Button,
  Group,
  Loader,
  Select,
  Skeleton,
  Stack,
  Text,
} from "@mantine/core";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { checkMonitorConnection } from "../api/monitor";
import type { DiscoveredMonitorDevice } from "../api/monitor";
import { monitorDevicesQuery, monitorSettingsQuery } from "../queries/monitor";
import { useConnectDevice } from "../hooks/useConnectDevice";
import { LineIcon } from "../../../shared/ui/LineIcon";

function deviceStatus(device: DiscoveredMonitorDevice | undefined) {
  if (!device) {
    return { sectionLabel: "当前保存设备", badgeLabel: "未发现", badgeColor: "gray" };
  }
  if (device.discoverySource === "mdns") {
    return { sectionLabel: "已发现设备", badgeLabel: "mDNS 发现", badgeColor: "teal" };
  }
  if (device.discoverySource === "udpBroadcast") {
    return { sectionLabel: "广播发现设备", badgeLabel: "UDP 广播", badgeColor: "teal" };
  }
  return { sectionLabel: "降级连接设备", badgeLabel: "直连在线", badgeColor: "teal" };
}

export function DeviceConnectPanel() {
  const settings = useQuery(monitorSettingsQuery);
  const devices = useQuery(monitorDevicesQuery);
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(
    null,
  );

  useEffect(() => {
    if (selectedDeviceId || !devices.data?.length) return;
    const matchingDevice = settings.data
      ? devices.data.find((device) => device.baseUrl === settings.data.baseUrl)
      : undefined;
    setSelectedDeviceId((matchingDevice ?? devices.data[0]).id);
  }, [devices.data, selectedDeviceId, settings.data]);

  const availableDevices = devices.data ?? [];
  const selectedDevice = availableDevices.find(
    (device) => device.id === selectedDeviceId,
  );
  const savedSettings = settings.data;
  const savedDeviceUnavailable = Boolean(
    savedSettings?.deviceId &&
      savedSettings.deviceId === selectedDeviceId &&
      !selectedDevice,
  );
  const deviceOptions = [
    ...availableDevices.map((device) => ({
      value: device.id,
      label:
        device.discoverySource === "udpBroadcast"
          ? `${device.name}（UDP 广播）`
          : device.name,
    })),
    ...(savedSettings && savedDeviceUnavailable
      ? [
          {
            value: savedSettings.deviceId,
            label: `${savedSettings.deviceName}（当前不可用）`,
          },
        ]
      : []),
  ];

  const connect = useConnectDevice();
  const status = deviceStatus(selectedDevice);

  const test = useMutation({
    mutationFn: (baseUrl: string) => checkMonitorConnection(baseUrl),
  });

  return (
    <Stack gap="lg">
      {settings.isPending || devices.isPending ? (
        <Skeleton height={72} radius="md" />
      ) : (
        <Select
          label="AIMonitor 设备"
          description="优先使用 mDNS，失败时自动通过每张网卡发送 UDP 广播"
          placeholder={
            devices.data?.length ? "选择发现的设备" : "未发现设备"
          }
          data={deviceOptions}
          value={selectedDeviceId}
          onChange={(value) => {
            test.reset();
            setSelectedDeviceId(value);
          }}
          renderOption={({ option }) => {
            const device = availableDevices.find(
              (item) => item.id === option.value,
            );
            return (
              <Group gap="sm" wrap="nowrap">
                <span
                  className={
                    device ? "device-status-dot" : "device-status-dot offline"
                  }
                />
                <div className="min-width-zero">
                  <Text size="sm" fw={600} truncate>
                    {option.label}
                  </Text>
                  {device && (
                    <Text size="xs" c="dimmed" truncate>
                      {device.baseUrl} · API v{device.apiVersion}
                    </Text>
                  )}
                </div>
              </Group>
            );
          }}
          rightSection={devices.isFetching ? <Loader size={16} /> : undefined}
          searchable
          allowDeselect={false}
          size="md"
          error={devices.error?.message ?? settings.error?.message}
        />
      )}

      {!devices.isPending && devices.data?.length === 0 && (
        <Alert color="yellow" variant="light">
          mDNS、UDP 广播和已保存地址均未发现可用设备，请确认设备已开机并接入同一局域网。
        </Alert>
      )}

      <div className="endpoint-preview">
        <Group justify="space-between" wrap="nowrap">
          <div>
            <Text size="xs" c="dimmed" tt="uppercase" fw={650}>
              {status.sectionLabel}
            </Text>
            <Text size="sm" fw={600} mt={4}>
              {selectedDevice?.name ?? settings.data?.deviceName ?? "尚未选择"}
            </Text>
            <Text size="xs" ff="monospace" c="dimmed" mt={3}>
              {selectedDevice?.baseUrl ?? settings.data?.baseUrl ?? "—"}
            </Text>
          </div>
          <Stack gap={6} align="flex-end">
            <Badge variant="light" color={status.badgeColor}>
              {status.badgeLabel}
            </Badge>
            <Text size="xs" c="dimmed">
              已发现 {devices.data?.length ?? 0} 台
            </Text>
          </Stack>
        </Group>
      </div>

      {test.data && (
        <Alert color={test.data.reachable ? "teal" : "red"}>
          {test.data.message}
        </Alert>
      )}
      {(connect.error || test.error) && (
        <Alert color="red">{(connect.error ?? test.error)?.message}</Alert>
      )}

      <Group justify="flex-end">
        <Button
          variant="default"
          leftSection={<LineIcon name="refresh" size={17} />}
          onClick={() => devices.refetch()}
          loading={devices.isFetching}
        >
          重新扫描
        </Button>
        <Button
          variant="default"
          onClick={() => selectedDevice && test.mutate(selectedDevice.baseUrl)}
          loading={test.isPending}
          disabled={!selectedDevice}
        >
          测试连接
        </Button>
        <Button
          onClick={() => {
            if (selectedDevice) connect.mutate(selectedDevice);
          }}
          loading={connect.isPending}
          leftSection={<LineIcon name="check" size={17} />}
          disabled={!selectedDevice}
        >
          连接设备
        </Button>
      </Group>
    </Stack>
  );
}
