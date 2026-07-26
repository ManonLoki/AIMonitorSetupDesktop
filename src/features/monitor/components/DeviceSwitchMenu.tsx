import { ActionIcon, Group, Loader, Menu, Stack, Text } from "@mantine/core";
import { useMonitorConnection } from "../hooks/useMonitorConnection";
import { useConnectDevice } from "../hooks/useConnectDevice";
import { LineIcon } from "../../../shared/ui/LineIcon";

interface DeviceSwitchMenuProps {
  collapsed?: boolean;
}

export function DeviceSwitchMenu({ collapsed = false }: DeviceSwitchMenuProps) {
  const {
    settings,
    connectedDevice,
    otherDevices,
    isPending,
    hasAvailableDevice,
  } = useMonitorConnection();
  const connect = useConnectDevice();
  const savedDevice = settings.data;
  const currentName = connectedDevice?.name ?? savedDevice?.deviceName;
  const currentBaseUrl = connectedDevice?.baseUrl ?? savedDevice?.baseUrl;
  const currentAvailable = Boolean(connectedDevice);

  if (isPending) return <Loader size={16} />;
  if (!hasAvailableDevice) {
    return collapsed ? (
      <ActionIcon
        variant="subtle"
        color="gray"
        size="lg"
        disabled
        aria-label="无可用设备"
      >
        <LineIcon name="server" size={18} />
      </ActionIcon>
    ) : (
      <Group gap="sm" wrap="nowrap" className="sidebar-device-button">
        <div className="status-dot offline" />
        <Text size="sm" c="dimmed" fw={600}>
          无可用设备
        </Text>
      </Group>
    );
  }

  return (
    <>
      {connect.isError && !collapsed && (
        <Text size="xs" c="red" px={4}>
          切换设备失败：{connect.error.message}
        </Text>
      )}
      <Menu shadow="md" width={220} position="top-start" withinPortal>
        <Menu.Target>
          {collapsed ? (
            <ActionIcon
              variant="subtle"
              color="gray"
              size="lg"
              className="sidebar-device-button-collapsed"
              aria-label={
                currentName
                  ? `当前设备：${currentName}${currentAvailable ? "" : "，未发现"}`
                  : "选择设备"
              }
            >
              <LineIcon name="server" size={18} />
              <span
                className={`status-dot device-indicator${
                  currentAvailable ? "" : " offline"
                }`}
              />
            </ActionIcon>
          ) : (
            <Group gap="sm" wrap="nowrap" className="sidebar-device-button">
              <div
                className={`status-dot${currentAvailable ? "" : " offline"}`}
              />
              <div className="min-width-zero">
                <Text size="sm" fw={600} truncate>
                  {currentName ?? "选择设备"}
                </Text>
                <Text size="xs" c="dimmed" truncate>
                  {currentAvailable ? currentBaseUrl : "当前设备未发现"}
                </Text>
              </div>
              <LineIcon name="chevronDown" size={14} />
            </Group>
          )}
        </Menu.Target>
        <Menu.Dropdown>
          <Menu.Label>当前设备</Menu.Label>
          {currentName ? (
            <Menu.Item disabled>
              <Stack gap={0}>
                <Text size="sm" fw={600}>
                  {currentName}
                  {!currentAvailable && "（未发现）"}
                </Text>
                <Text size="xs" c="dimmed">
                  {currentBaseUrl}
                </Text>
              </Stack>
            </Menu.Item>
          ) : (
            <Menu.Item disabled>尚未连接设备</Menu.Item>
          )}
          <Menu.Divider />
          <Menu.Label>切换设备</Menu.Label>
          {otherDevices.length === 0 ? (
            <Menu.Item disabled>未发现其他可连接设备</Menu.Item>
          ) : (
            otherDevices.map((device) => (
              <Menu.Item
                key={device.id}
                disabled={connect.isPending}
                onClick={() => connect.mutate(device)}
                rightSection={
                  connect.isPending &&
                  connect.variables?.id === device.id ? (
                    <Loader size={14} />
                  ) : undefined
                }
              >
                <Stack gap={0}>
                  <Text size="sm">{device.name}</Text>
                  <Text size="xs" c="dimmed">
                    {device.baseUrl}
                  </Text>
                </Stack>
              </Menu.Item>
            ))
          )}
        </Menu.Dropdown>
      </Menu>
    </>
  );
}
