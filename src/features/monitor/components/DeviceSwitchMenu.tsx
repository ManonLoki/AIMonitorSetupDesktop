import { ActionIcon, Group, Loader, Menu, Stack, Text } from "@mantine/core";
import { useMonitorConnection } from "../hooks/useMonitorConnection";
import { useConnectDevice } from "../hooks/useConnectDevice";
import { LineIcon } from "../../../shared/ui/LineIcon";
import { useI18n } from "../../../shared/i18n";

// collapsed 为 true 时使用侧边栏收起态的图标按钮样式，否则展示完整的设备信息条
interface DeviceSwitchMenuProps {
  collapsed?: boolean;
}

export function DeviceSwitchMenu({ collapsed = false }: DeviceSwitchMenuProps) {
  const { t } = useI18n();
  // 从 Rust 原子快照中取出当前设备与其他可切换设备。
  const {
    snapshot,
    connectedDevice,
    otherDevices,
    isPending,
    hasAvailableDevice,
  } = useMonitorConnection();
  // 用于切换/连接设备的 mutation
  const connect = useConnectDevice();
  const savedDevice = snapshot.data?.savedDevice;
  // 优先展示当前已连接设备的信息，若未连接则回退展示已保存的设备信息
  const currentName = connectedDevice?.name ?? savedDevice?.name;
  const currentBaseUrl = connectedDevice?.baseUrl ?? savedDevice?.baseUrl;
  const currentAvailable = Boolean(connectedDevice);

  // 设备状态尚在加载时，仅展示一个小的加载指示器
  if (isPending) return <Loader size={16} />;
  // 完全没有可用设备时，根据是否收起渲染禁用态的图标按钮或提示文案
  if (!hasAvailableDevice && !savedDevice) {
    return collapsed ? (
      <ActionIcon
        variant="subtle"
        color="gray"
        size="lg"
        disabled
        aria-label={t("device.none")}
      >
        <LineIcon name="server" size={18} />
      </ActionIcon>
    ) : (
      <Group gap="sm" wrap="nowrap" className="sidebar-device-button">
        <div className="status-dot offline" />
        <Text size="sm" c="dimmed" fw={600}>
          {t("device.none")}
        </Text>
      </Group>
    );
  }

  return (
    <>
      {/* 切换设备失败时，在展开态下展示错误提示 */}
      {connect.isError && !collapsed && (
        <Text size="xs" c="red" px={4}>
          {t("device.switchFailed", { message: connect.error.message })}
        </Text>
      )}
      <Menu shadow="md" width={220} position="top-start" withinPortal>
        <Menu.Target>
          {collapsed ? (
            // 收起态：仅展示服务器图标，右上角小圆点标记当前设备是否在线
            <ActionIcon
              variant="subtle"
              color="gray"
              size="lg"
              className="sidebar-device-button-collapsed"
              aria-label={
                currentName
                  ? t("device.currentNamed", { name: currentName, suffix: currentAvailable ? "" : t("device.notDiscoveredSuffix") })
                  : t("device.choose")
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
            // 展开态：展示状态圆点、设备名称、地址（或未发现提示）以及下拉箭头
            <Group gap="sm" wrap="nowrap" className="sidebar-device-button">
              <div
                className={`status-dot${currentAvailable ? "" : " offline"}`}
              />
              <div className="min-width-zero">
                <Text size="sm" fw={600} truncate>
                  {currentName ?? t("device.choose")}
                </Text>
                <Text size="xs" c="dimmed" truncate>
                  {currentAvailable ? currentBaseUrl : t("device.notDiscovered")}
                </Text>
              </div>
              <LineIcon name="chevronDown" size={14} />
            </Group>
          )}
        </Menu.Target>
        <Menu.Dropdown>
          <Menu.Label>{t("device.current")}</Menu.Label>
          {/* 当前设备信息条目：有设备名则展示名称与地址（未发现时附加提示），否则展示未连接文案 */}
          {currentName ? (
            <Menu.Item disabled>
              <Stack gap={0}>
                <Text size="sm" fw={600}>
                  {currentName}
                  {!currentAvailable && t("device.notDiscoveredParenthetical")}
                </Text>
                <Text size="xs" c="dimmed">
                  {currentBaseUrl}
                </Text>
              </Stack>
            </Menu.Item>
          ) : (
            <Menu.Item disabled>{t("device.notConnected")}</Menu.Item>
          )}
          <Menu.Divider />
          <Menu.Label>{t("device.switch")}</Menu.Label>
          {/* 其他可切换设备列表：为空时展示提示，否则逐个渲染可点击切换的设备条目 */}
          {otherDevices.length === 0 ? (
            <Menu.Item disabled>{t("device.noOthers")}</Menu.Item>
          ) : (
            otherDevices.map((device) => (
              <Menu.Item
                key={device.id}
                disabled={connect.isPending}
                onClick={() => connect.mutate(device)}
                rightSection={
                  // 正在连接当前这台设备时，在条目右侧展示加载指示器
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
