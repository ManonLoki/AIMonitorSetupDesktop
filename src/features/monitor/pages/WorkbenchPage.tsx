// 引入 Mantine UI 组件：提示、徽标、按钮、卡片、代码片段展示、分组/网格/堆叠布局、文本、标题
import {
  Alert,
  Badge,
  Button,
  Card,
  Code,
  Group,
  SimpleGrid,
  Stack,
  Text,
  Title,
} from "@mantine/core";
// 引入 TanStack Query 的 query hook
import { useQuery } from "@tanstack/react-query";
// 引入被发现设备的类型定义
import type { DiscoveredMonitorDevice } from "../api/monitor";
// 引入查询键与预定义的查询配置
import {
  monitorDevicesQuery,
  monitorSettingsQuery,
  runtimeOverviewQuery,
} from "../queries/monitor";
// 引入通用图标组件
import { LineIcon } from "../../../shared/ui/LineIcon";
import { useI18n } from "../../../shared/i18n";

export function WorkbenchPage() {
  const { t } = useI18n();
  const discoverySourceLabel: Record<DiscoveredMonitorDevice["discoverySource"], string> = {
    mdns: t("device.mdns"),
    udpBroadcast: t("device.udp"),
    savedAddress: t("device.savedAddress"),
  };
  // 查询运行时概览信息（含 hook 中继状态），对接 get_runtime_overview 命令，每 3 秒自动轮询一次
  const runtime = useQuery(runtimeOverviewQuery);
  // 查询当前监控设置（用于判断哪个设备是“当前连接”），对接 get_monitor_settings 命令
  const settings = useQuery(monitorSettingsQuery);
  // 查询局域网内已发现的设备列表，对接 discover_monitor_devices 命令，10 秒内视为新鲜
  const devices = useQuery(monitorDevicesQuery);
  // 从运行时概览中取出 hook 中继状态，便于下方多处引用
  const relay = runtime.data?.hookRelay;

  return (
    <Stack gap="md">
      {/* 运行时概览查询本身的错误提示 */}
      {runtime.error && <Alert color="red">{runtime.error.message}</Alert>}

      {/* 在线设备卡片：展示已发现设备列表，并支持手动强制重新检查 */}
      <Card withBorder radius="lg" p="md" className="surface-card">
        <Stack gap="md">
          {/* 标题、说明文案与“强制重新检查”按钮 */}
          <Group justify="space-between" align="flex-start">
            <div>
              <Title order={3}>{t("workbench.onlineTitle")}</Title>
              <Text size="sm" c="dimmed" mt={4}>
                {t("workbench.onlineDescription")}
              </Text>
            </div>
            <Button
              variant="default"
              leftSection={<LineIcon name="refresh" size={17} />}
              // 手动触发一次设备发现，强制刷新在线设备列表
              onClick={() => devices.refetch()}
              loading={devices.isFetching}
            >
              {t("workbench.forceCheck")}
            </Button>
          </Group>

          {/* 设备发现查询自身的错误提示 */}
          {devices.error && <Alert color="red">{devices.error.message}</Alert>}

          {/* 查询完成但未发现任何设备时的提示 */}
          {!devices.isPending && devices.data?.length === 0 && (
            <Alert color="yellow" variant="light">
              {t("workbench.noOnline")}
            </Alert>
          )}

          {/* 已发现设备列表：逐个渲染设备名称（当前连接设备额外标注）、访问地址、API 版本与发现来源徽标 */}
          {(devices.data?.length ?? 0) > 0 && (
            <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
              {devices.data?.map((device) => (
                <div className="endpoint-preview" key={device.id}>
                  <Group justify="space-between" wrap="nowrap">
                    <Group gap="sm" wrap="nowrap">
                      <span className="device-status-dot" />
                      <div className="min-width-zero">
                        <Text size="sm" fw={600} truncate>
                          {device.name}
                          {/* 当前已连接设备的额外标注 */}
                          {device.id === settings.data?.deviceId && (
                            <Text component="span" size="xs" c="dimmed">
                              {" "}
                              · {t("device.currentConnection")}
                            </Text>
                          )}
                        </Text>
                        <Text size="xs" c="dimmed" truncate>
                          {device.baseUrl} · API v{device.apiVersion}
                        </Text>
                      </div>
                    </Group>
                    {/* 设备发现来源徽标 */}
                    <Badge variant="light" color="teal">
                      {discoverySourceLabel[device.discoverySource]}
                    </Badge>
                  </Group>
                </div>
              ))}
            </SimpleGrid>
          )}
        </Stack>
      </Card>

      {/* 本机 Hook 中继卡片：展示中继监听状态、各项统计指标、最近一次事件与最近一次错误 */}
      <Card withBorder radius="lg" p="md" className="surface-card">
        <Stack gap="md">
          {/* 标题、说明文案与监听状态徽标 */}
          <Group justify="space-between" align="flex-start">
            <div>
              <Title order={3}>{t("workbench.relayTitle")}</Title>
              <Text size="sm" c="dimmed" mt={4}>
                {t("workbench.relayDescription")}
              </Text>
            </div>
            <Badge color={relay?.listening ? "green" : "red"} variant="light">
              {relay?.listening ? t("workbench.listening") : t("workbench.notRunning")}
            </Badge>
          </Group>

          {/* 中继统计指标网格：接收数、转发成功/失败数、等待处理数、时序抑制数 */}
          <SimpleGrid cols={{ base: 2, sm: 3 }}>
            <RelayMetric label={t("workbench.received")} value={relay?.receivedCount ?? 0} />
            <RelayMetric label={t("workbench.forwarded")} value={relay?.forwardedCount ?? 0} />
            <RelayMetric label={t("workbench.failed")} value={relay?.failedCount ?? 0} />
            <RelayMetric label={t("workbench.pending")} value={relay?.pendingCount ?? 0} />
            <RelayMetric
              label={t("workbench.suppressed")}
              value={relay?.suppressedCount ?? 0}
            />
          </SimpleGrid>

          {/* 最近一次事件的工具、hook 类型与对应行为（若无行为则说明是释放位置） */}
          {relay?.lastHookType && (
            <Text size="sm">
              {t("workbench.latestEvent")} <Code>{relay.lastTool}</Code> /{" "}
              <Code>{relay.lastHookType}</Code>
              {relay.lastBehavior
                ? ` · ${t("workbench.currentBehavior", { behavior: t(`behavior.${relay.lastBehavior}`) })}`
                : ` · ${t("workbench.released")}`}
            </Text>
          )}

          {/* 最近一次转发失败时展示错误详情 */}
          {relay?.lastError && (
            <Alert color="red" title={t("workbench.latestFailure")}>
              {relay.lastError}
            </Alert>
          )}
        </Stack>
      </Card>
    </Stack>
  );
}

// 中继统计指标小卡片：展示一个标签与对应数值
function RelayMetric({ label, value }: { label: string; value: number }) {
  return (
    <div className="endpoint-preview">
      <Text size="xs" c="dimmed">
        {label}
      </Text>
      <Text fw={700} size="xl">
        {value}
      </Text>
    </div>
  );
}
