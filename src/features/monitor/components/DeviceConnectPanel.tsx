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
import { monitorDevicesQuery } from "../queries/monitor";
import { useConnectDevice } from "../hooks/useConnectDevice";
import { LineIcon } from "../../../shared/ui/LineIcon";
import { describeError, useI18n } from "../../../shared/i18n";

// 根据设备信息推导出面板展示所需的分组标题、徽标文案与徽标颜色
function deviceStatus(device: DiscoveredMonitorDevice | undefined, t: ReturnType<typeof useI18n>["t"]) {
  // 未选中任何设备时，展示"当前保存设备"分组，徽标标记为未发现
  if (!device) {
    return { sectionLabel: t("device.savedSection"), badgeLabel: t("device.notFoundBadge"), badgeColor: "gray" };
  }
  // 设备通过 mDNS 发现
  if (device.discoverySource === "mdns") {
    return { sectionLabel: t("device.discoveredSection"), badgeLabel: t("device.mdns"), badgeColor: "teal" };
  }
  // 设备通过 UDP 广播发现
  if (device.discoverySource === "udpBroadcast") {
    return { sectionLabel: t("device.broadcastSection"), badgeLabel: t("device.udp"), badgeColor: "teal" };
  }
  // 其余情况视为通过已保存地址直接降级连接
  return { sectionLabel: t("device.fallbackSection"), badgeLabel: t("device.direct"), badgeColor: "teal" };
}

export function DeviceConnectPanel() {
  const { t } = useI18n();
  // 拉取 Rust 同一次状态读取生成的设备快照。
  const snapshot = useQuery(monitorDevicesQuery);
  // 当前在下拉框中选中的设备 id
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(
    null,
  );

  const backendSelectedDeviceId = snapshot.data?.selectedDeviceId;
  // 初次加载或 Rust 发生自动/手动切换时，同步其明确选择；普通刷新不会
  // 覆盖用户尚未提交的下拉框选择。
  useEffect(() => {
    setSelectedDeviceId(backendSelectedDeviceId || null);
  }, [backendSelectedDeviceId]);

  // 当前可选设备列表（未加载完成时为空数组）
  const availableDevices = snapshot.data?.devices ?? [];
  // 根据选中 id 在可用设备中查找完整设备对象
  const selectedDevice = availableDevices.find(
    (device) => device.id === selectedDeviceId,
  );
  const savedDevice = snapshot.data?.savedDevice;
  const selectedSavedDevice =
    savedDevice?.id === selectedDeviceId ? savedDevice : undefined;
  // 组装下拉框选项：先列出所有可用设备，UDP 广播来源的设备名附加标记；
  // 若已保存设备当前不可用，则额外追加一项供用户看到但标注"当前不可用"
  const deviceOptions = [
    ...availableDevices.map((device) => ({
      value: device.id,
      label:
        device.discoverySource === "udpBroadcast"
          ? `${device.name} (${t("device.udp")})`
          : device.name,
    })),
    ...(savedDevice && !snapshot.data?.currentDevice
      ? [
          {
            value: savedDevice.id,
            label: `${savedDevice.name} (${t("device.notFoundBadge")})`,
          },
        ]
      : []),
  ];

  // 连接设备的 mutation
  const connect = useConnectDevice();
  // 根据当前选中设备计算展示状态
  const status = deviceStatus(selectedDevice, t);

  // 测试连接的 mutation：向指定 baseUrl 发起可达性检测
  const test = useMutation({
    mutationFn: (baseUrl: string) => checkMonitorConnection(baseUrl),
  });

  return (
    <Stack gap="lg">
      {/* 设置或设备列表尚在加载时展示骨架屏，加载完成后展示设备选择下拉框 */}
      {snapshot.isPending ? (
        <Skeleton height={72} radius="md" />
      ) : (
        <Select
          label={t("device.selectorLabel")}
          description={t("device.selectorDescription")}
          placeholder={
            availableDevices.length
              ? t("device.selectorPlaceholder")
              : t("device.noneFound")
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
          rightSection={snapshot.isFetching ? <Loader size={16} /> : undefined}
          searchable
          allowDeselect={false}
          size="md"
          error={snapshot.error ? describeError(snapshot.error, t) : undefined}
        />
      )}

      {/* 加载完成但没有发现任何设备时，提示用户检查设备联网状态 */}
      {!snapshot.isPending && availableDevices.length === 0 && (
        <Alert color="yellow" variant="light">
          {t("device.noneFoundDescription")}
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
              {selectedDevice?.name ??
                selectedSavedDevice?.name ??
                t("device.notSelected")}
            </Text>
            <Text size="xs" ff="monospace" c="dimmed" mt={3}>
              {selectedDevice?.baseUrl ?? selectedSavedDevice?.baseUrl ?? "—"}
            </Text>
          </div>
          <Stack gap={6} align="flex-end">
            <Badge variant="light" color={status.badgeColor}>
              {status.badgeLabel}
            </Badge>
            <Text size="xs" c="dimmed">
              {t("device.foundCount", { count: availableDevices.length })}
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
        <Alert color="red">{describeError(connect.error ?? test.error, t)}</Alert>
      )}

      {/* 操作按钮组：重新扫描设备、测试连接、正式连接设备 */}
      <Group justify="flex-end">
        <Button
          variant="default"
          leftSection={<LineIcon name="refresh" size={17} />}
          onClick={() => snapshot.refetch()}
          loading={snapshot.isFetching}
        >
          {t("device.rescan")}
        </Button>
        <Button
          variant="default"
          onClick={() => selectedDevice && test.mutate(selectedDevice.baseUrl)}
          loading={test.isPending}
          disabled={!selectedDevice}
        >
          {t("device.testConnection")}
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
          {t("device.connect")}
        </Button>
      </Group>
    </Stack>
  );
}
