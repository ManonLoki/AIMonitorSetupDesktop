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

// 根据设备信息推导出面板展示所需的分组标题、徽标文案与徽标颜色
function deviceStatus(device: DiscoveredMonitorDevice | undefined) {
  // 未选中任何设备时，展示"当前保存设备"分组，徽标标记为未发现
  if (!device) {
    return { sectionLabel: "当前保存设备", badgeLabel: "未发现", badgeColor: "gray" };
  }
  // 设备通过 mDNS 发现
  if (device.discoverySource === "mdns") {
    return { sectionLabel: "已发现设备", badgeLabel: "mDNS 发现", badgeColor: "teal" };
  }
  // 设备通过 UDP 广播发现
  if (device.discoverySource === "udpBroadcast") {
    return { sectionLabel: "广播发现设备", badgeLabel: "UDP 广播", badgeColor: "teal" };
  }
  // 其余情况视为通过已保存地址直接降级连接
  return { sectionLabel: "降级连接设备", badgeLabel: "直连在线", badgeColor: "teal" };
}

export function DeviceConnectPanel() {
  // 拉取已保存的设备设置
  const settings = useQuery(monitorSettingsQuery);
  // 拉取当前发现到的设备列表
  const devices = useQuery(monitorDevicesQuery);
  // 当前在下拉框中选中的设备 id
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(
    null,
  );

  // 设备列表加载完成且尚未手动选择时，自动匹配已保存设备或默认选中第一台
  useEffect(() => {
    if (selectedDeviceId || !devices.data?.length) return;
    const matchingDevice = settings.data
      ? devices.data.find((device) => device.baseUrl === settings.data.baseUrl)
      : undefined;
    setSelectedDeviceId((matchingDevice ?? devices.data[0]).id);
  }, [devices.data, selectedDeviceId, settings.data]);

  // 当前可选设备列表（未加载完成时为空数组）
  const availableDevices = devices.data ?? [];
  // 根据选中 id 在可用设备中查找完整设备对象
  const selectedDevice = availableDevices.find(
    (device) => device.id === selectedDeviceId,
  );
  const savedSettings = settings.data;
  // 判断当前选中的正是已保存设备，但该设备已不在可用列表中（即离线/不可达）
  const savedDeviceUnavailable = Boolean(
    savedSettings?.deviceId &&
      savedSettings.deviceId === selectedDeviceId &&
      !selectedDevice,
  );
  // 组装下拉框选项：先列出所有可用设备，UDP 广播来源的设备名附加标记；
  // 若已保存设备当前不可用，则额外追加一项供用户看到但标注"当前不可用"
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

  // 连接设备的 mutation
  const connect = useConnectDevice();
  // 根据当前选中设备计算展示状态
  const status = deviceStatus(selectedDevice);

  // 测试连接的 mutation：向指定 baseUrl 发起可达性检测
  const test = useMutation({
    mutationFn: (baseUrl: string) => checkMonitorConnection(baseUrl),
  });

  return (
    <Stack gap="lg">
      {/* 设置或设备列表尚在加载时展示骨架屏，加载完成后展示设备选择下拉框 */}
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
            // 切换设备选项时重置上一次的测试连接结果
            test.reset();
            setSelectedDeviceId(value);
          }}
          renderOption={({ option }) => {
            // 自定义每个下拉选项的渲染：状态圆点 + 名称 + 地址与 API 版本
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

      {/* 加载完成但没有发现任何设备时，提示用户检查设备联网状态 */}
      {!devices.isPending && devices.data?.length === 0 && (
        <Alert color="yellow" variant="light">
          mDNS、UDP 广播和已保存地址均未发现可用设备，请确认设备已开机并接入同一局域网。
        </Alert>
      )}

      {/* 当前选中设备的详情预览：分组标题、设备名、地址、状态徽标与发现数量 */}
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

      {/* 测试连接结果：可达显示为绿色提示，不可达显示为红色提示 */}
      {test.data && (
        <Alert color={test.data.reachable ? "teal" : "red"}>
          {test.data.message}
        </Alert>
      )}
      {/* 连接或测试过程中出现的错误信息 */}
      {(connect.error || test.error) && (
        <Alert color="red">{(connect.error ?? test.error)?.message}</Alert>
      )}

      {/* 操作按钮组：重新扫描设备、测试连接、正式连接设备 */}
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
            // 点击连接后，若已选中设备则触发连接 mutation
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
