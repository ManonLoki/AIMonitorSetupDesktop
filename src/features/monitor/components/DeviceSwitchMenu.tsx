import { ActionIcon, Group, Loader, Menu, Stack, Text } from "@mantine/core";
import { useMonitorConnection } from "../hooks/useMonitorConnection";
import { useConnectDevice } from "../hooks/useConnectDevice";
import { LineIcon } from "../../../shared/ui/LineIcon";

interface DeviceSwitchMenuProps {
  collapsed?: boolean;
}

export function DeviceSwitchMenu({ collapsed = false }: DeviceSwitchMenuProps) {
  const { settings, connectedDevice, otherDevices } = useMonitorConnection();
  const connect = useConnectDevice();

  if (!connectedDevice) return null;

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
              aria-label={`当前设备：${connectedDevice.name}`}
            >
              <LineIcon name="server" size={18} />
              <span className="status-dot device-indicator" />
            </ActionIcon>
          ) : (
            <Group gap="sm" wrap="nowrap" className="sidebar-device-button">
              <div className="status-dot" />
              <div className="min-width-zero">
                <Text size="sm" fw={600} truncate>
                  {connectedDevice.name}
                </Text>
                <Text size="xs" c="dimmed" truncate>
                  {connectedDevice.baseUrl}
                </Text>
              </div>
              <LineIcon name="chevronDown" size={14} />
            </Group>
          )}
        </Menu.Target>
        <Menu.Dropdown>
          <Menu.Label>当前设备</Menu.Label>
          <Menu.Item disabled>
            <Stack gap={0}>
              <Text size="sm" fw={600}>
                {connectedDevice.name}
              </Text>
              <Text size="xs" c="dimmed">
                {connectedDevice.baseUrl}
              </Text>
            </Stack>
          </Menu.Item>
          <Menu.Divider />
          <Menu.Label>切换设备</Menu.Label>
          {otherDevices.length === 0 ? (
            <Menu.Item disabled>未发现其他可连接设备</Menu.Item>
          ) : (
            otherDevices.map((device) => (
              <Menu.Item
                key={device.id}
                disabled={connect.isPending}
                onClick={() =>
                  settings.data &&
                  connect.mutate({
                    device,
                    username: settings.data.username,
                  })
                }
                rightSection={
                  connect.isPending &&
                  connect.variables?.device.id === device.id ? (
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
