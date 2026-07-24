import {
  Alert,
  Badge,
  Button,
  Card,
  Group,
  Loader,
  Select,
  Skeleton,
  Stack,
  Tabs,
  Text,
  TextInput,
  Title,
} from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import {
  checkMonitorConnection,
  saveManualMonitorSettings,
  saveMonitorSettings,
} from "../api/monitor";
import type { DiscoveredMonitorDevice } from "../api/monitor";
import {
  monitorDevicesQuery,
  monitorKeys,
  monitorSettingsQuery,
} from "../queries/monitor";
import { LineIcon } from "../../../shared/ui/LineIcon";

export function SettingsPage() {
  const queryClient = useQueryClient();
  const settings = useQuery(monitorSettingsQuery);
  const devices = useQuery(monitorDevicesQuery);
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(
    null,
  );
  const [username, setUsername] = useState("");
  const [manualName, setManualName] = useState("");
  const [manualBaseUrl, setManualBaseUrl] = useState("");
  const [saveAttempted, setSaveAttempted] = useState(false);

  useEffect(() => {
    if (settings.data) {
      setUsername(settings.data.username);
      setSelectedDeviceId(
        (current) => current ?? (settings.data.deviceId || null),
      );
      if (settings.data.deviceId) {
        setManualName((current) => current || settings.data.deviceName);
        setManualBaseUrl((current) => current || settings.data.baseUrl);
      }
    }
  }, [settings.data]);

  useEffect(() => {
    if (selectedDeviceId || !settings.data || !devices.data) return;
    const matchingDevice = devices.data.find(
      (device) => device.baseUrl === settings.data.baseUrl,
    );
    if (matchingDevice) setSelectedDeviceId(matchingDevice.id);
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
    ...(availableDevices.map((device) => ({
      value: device.id,
      label:
        device.discoverySource === "manualAddress"
          ? `${device.name}（手动）`
          : device.discoverySource === "udpBroadcast"
            ? `${device.name}（UDP 广播）`
            : device.name,
    }))),
    ...(savedSettings && savedDeviceUnavailable
      ? [
          {
            value: savedSettings.deviceId,
            label: `${savedSettings.deviceName}（当前不可用）`,
          },
        ]
      : []),
  ];

  const save = useMutation({
    mutationFn: (device: DiscoveredMonitorDevice) =>
      saveMonitorSettings(device, username),
    onSuccess: (data) => {
      queryClient.setQueryData(monitorKeys.settings(), data);
    },
  });

  const test = useMutation({
    mutationFn: (baseUrl: string) => checkMonitorConnection(baseUrl),
  });

  const manualSave = useMutation({
    mutationFn: () =>
      saveManualMonitorSettings(manualName, manualBaseUrl, username),
    onSuccess: (data) => {
      queryClient.setQueryData(monitorKeys.settings(), data);
    },
  });

  return (
    <Stack gap={28} maw={860}>
      <PageHeading
        title="设置"
        description="配置用户名并选择局域网中的 AIMonitor 设备。"
      />

      <Card withBorder className="surface-card" padding={0}>
        <div className="settings-card-header">
          <div className="settings-icon">
            <LineIcon name="ai" size={22} />
          </div>
          <Text fw={650}>用户名</Text>
        </div>
        <Stack p={24}>
          <TextInput
            label="显示用户名"
            placeholder="输入设备上显示的名称"
            value={username}
            onChange={(event) => {
              save.reset();
              manualSave.reset();
              setUsername(event.currentTarget.value);
            }}
            error={
              saveAttempted && !username.trim()
                ? "保存设置前请输入显示用户名"
                : undefined
            }
            required
            size="md"
          />
        </Stack>
      </Card>

      <Card withBorder className="surface-card" padding={0}>
        <div className="settings-card-header">
          <div className="settings-icon">
            <LineIcon name="server" size={22} />
          </div>
          <div>
            <Text fw={650}>目标设备</Text>
            <Text size="sm" c="dimmed">
              从局域网中发现 AIMonitor，并选择状态与图片的目标设备
            </Text>
          </div>
        </div>

        <Tabs
          defaultValue="automatic"
          onChange={() => test.reset()}
          keepMounted={false}
        >
          <Tabs.List px={24} pt={12}>
            <Tabs.Tab value="automatic">自动发现</Tabs.Tab>
            <Tabs.Tab value="manual">手动维护</Tabs.Tab>
          </Tabs.List>

          <Tabs.Panel value="automatic">
            <Stack p={24} gap="lg">
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
                            device
                              ? "device-status-dot"
                              : "device-status-dot offline"
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
                  rightSection={
                    devices.isFetching ? <Loader size={16} /> : undefined
                  }
                  searchable
                  allowDeselect={false}
                  size="md"
                  error={devices.error?.message ?? settings.error?.message}
                />
              )}

              {!devices.isPending && devices.data?.length === 0 && (
                <Alert color="yellow" variant="light">
                  mDNS、UDP 广播和已保存地址均未发现可用设备；可切换到“手动维护”。
                </Alert>
              )}

              <div className="endpoint-preview">
                <Group justify="space-between" wrap="nowrap">
                  <div>
                    <Text size="xs" c="dimmed" tt="uppercase" fw={650}>
                      {selectedDevice?.discoverySource === "mdns"
                        ? "已发现设备"
                        : selectedDevice?.discoverySource === "udpBroadcast"
                          ? "广播发现设备"
                          : selectedDevice
                            ? "降级连接设备"
                            : "当前保存设备"}
                    </Text>
                    <Text size="sm" fw={600} mt={4}>
                      {selectedDevice?.name ??
                        settings.data?.deviceName ??
                        "尚未选择"}
                    </Text>
                    <Text size="xs" ff="monospace" c="dimmed" mt={3}>
                      {selectedDevice?.baseUrl ??
                        settings.data?.baseUrl ??
                        "—"}
                    </Text>
                  </div>
                  <Stack gap={6} align="flex-end">
                    <Badge
                      variant="light"
                      color={selectedDevice ? "teal" : "gray"}
                    >
                      {selectedDevice?.discoverySource === "mdns"
                        ? "mDNS 发现"
                        : selectedDevice?.discoverySource === "udpBroadcast"
                          ? "UDP 广播"
                          : selectedDevice
                            ? "直连在线"
                            : "未发现"}
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
              {(save.error || test.error) && (
                <Alert color="red">
                  {(save.error ?? test.error)?.message}
                </Alert>
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
                  onClick={() =>
                    selectedDevice && test.mutate(selectedDevice.baseUrl)
                  }
                  loading={test.isPending}
                  disabled={!selectedDevice}
                >
                  测试连接
                </Button>
                <Button
                  onClick={() => {
                    setSaveAttempted(true);
                    if (selectedDevice && username.trim()) {
                      save.mutate(selectedDevice);
                    }
                  }}
                  loading={save.isPending}
                  leftSection={<LineIcon name="check" size={17} />}
                  disabled={!selectedDevice}
                >
                  保存设置
                </Button>
              </Group>
            </Stack>
          </Tabs.Panel>

          <Tabs.Panel value="manual">
            <Stack p={24} gap="lg">
              <TextInput
                label="设备名称"
                description="用于在应用中识别这台设备"
                placeholder="例如：办公室显示器"
                value={manualName}
                onChange={(event) => {
                  manualSave.reset();
                  setManualName(event.currentTarget.value);
                }}
                size="md"
              />
              <TextInput
                label="服务地址"
                description="完整填写协议、IP 或主机名以及端口"
                placeholder="http://192.168.1.100:8080"
                value={manualBaseUrl}
                onChange={(event) => {
                  manualSave.reset();
                  setManualBaseUrl(event.currentTarget.value);
                }}
                size="md"
              />

              <Alert color="blue" variant="light">
                手动设置允许在设备暂时离线时保存。建议先测试连接；保存后所有设备请求都会使用该地址。
              </Alert>

              {test.data && (
                <Alert color={test.data.reachable ? "teal" : "red"}>
                  {test.data.message}
                </Alert>
              )}
              {(manualSave.error || test.error) && (
                <Alert color="red">
                  {(manualSave.error ?? test.error)?.message}
                </Alert>
              )}

              <Group justify="flex-end">
                <Button
                  variant="default"
                  onClick={() => test.mutate(manualBaseUrl)}
                  loading={test.isPending}
                  disabled={!manualBaseUrl.trim()}
                >
                  测试连接
                </Button>
                <Button
                  onClick={() => {
                    setSaveAttempted(true);
                    if (username.trim()) {
                      manualSave.mutate();
                    }
                  }}
                  loading={manualSave.isPending}
                  leftSection={<LineIcon name="check" size={17} />}
                  disabled={!manualName.trim() || !manualBaseUrl.trim()}
                >
                  保存手动设置
                </Button>
              </Group>
            </Stack>
          </Tabs.Panel>
        </Tabs>
      </Card>

      <Alert
        variant="light"
        color="blue"
        title="局域网使用提示"
        icon={<LineIcon name="settings" size={18} />}
      >
        设备接口目前不包含鉴权与 TLS，请仅在可信局域网内使用，不要将端口直接暴露到公网。
      </Alert>
    </Stack>
  );
}

function PageHeading({
  title,
  description,
}: {
  title: string;
  description: string;
}) {
  return (
    <div>
      <Title order={1} className="page-title">
        {title}
      </Title>
      <Text c="dimmed" mt={5}>
        {description}
      </Text>
    </div>
  );
}
