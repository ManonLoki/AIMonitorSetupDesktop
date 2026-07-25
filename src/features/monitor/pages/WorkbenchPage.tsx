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
import { useQuery } from "@tanstack/react-query";
import type { DiscoveredMonitorDevice } from "../api/monitor";
import {
  monitorDevicesQuery,
  monitorSettingsQuery,
  runtimeOverviewQuery,
} from "../queries/monitor";
import { LineIcon } from "../../../shared/ui/LineIcon";

const discoverySourceLabel: Record<DiscoveredMonitorDevice["discoverySource"], string> = {
  mdns: "mDNS 发现",
  udpBroadcast: "UDP 广播",
  savedAddress: "已保存地址",
};

export function WorkbenchPage() {
  const runtime = useQuery(runtimeOverviewQuery);
  const settings = useQuery(monitorSettingsQuery);
  const devices = useQuery(monitorDevicesQuery);
  const relay = runtime.data?.hookRelay;

  return (
    <Stack gap="md">
      {runtime.error && <Alert color="red">{runtime.error.message}</Alert>}

      <Card withBorder radius="lg" p="md" className="surface-card">
        <Stack gap="md">
          <Group justify="space-between" align="flex-start">
            <div>
              <Title order={3}>在线设备</Title>
              <Text size="sm" c="dimmed" mt={4}>
                按设置页配置的间隔自动刷新，也可以立即强制重新检查一次。
              </Text>
            </div>
            <Button
              variant="default"
              leftSection={<LineIcon name="refresh" size={17} />}
              onClick={() => devices.refetch()}
              loading={devices.isFetching}
            >
              强制重新检查
            </Button>
          </Group>

          {devices.error && <Alert color="red">{devices.error.message}</Alert>}

          {!devices.isPending && devices.data?.length === 0 && (
            <Alert color="yellow" variant="light">
              暂未发现在线设备，请确认设备已开机并接入同一局域网。
            </Alert>
          )}

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
                          {device.id === settings.data?.deviceId && (
                            <Text component="span" size="xs" c="dimmed">
                              {" "}
                              · 当前连接
                            </Text>
                          )}
                        </Text>
                        <Text size="xs" c="dimmed" truncate>
                          {device.baseUrl} · API v{device.apiVersion}
                        </Text>
                      </div>
                    </Group>
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

      <Card withBorder radius="lg" p="md" className="surface-card">
        <Stack gap="md">
          <Group justify="space-between" align="flex-start">
            <div>
              <Title order={3}>本机 Hook 中继</Title>
              <Text size="sm" c="dimmed" mt={4}>
                AI 工具只连接本机，中继按 AI 标识遍历所有已配置设备。
              </Text>
            </div>
            <Badge color={relay?.listening ? "green" : "red"} variant="light">
              {relay?.listening ? "监听中" : "未运行"}
            </Badge>
          </Group>

          <SimpleGrid cols={{ base: 2, sm: 3 }}>
            <RelayMetric label="已接收事件" value={relay?.receivedCount ?? 0} />
            <RelayMetric label="设备转发成功" value={relay?.forwardedCount ?? 0} />
            <RelayMetric label="设备转发失败" value={relay?.failedCount ?? 0} />
            <RelayMetric label="等待处理" value={relay?.pendingCount ?? 0} />
            <RelayMetric label="自动重试" value={relay?.retriedCount ?? 0} />
            <RelayMetric label="时序抑制" value={relay?.suppressedCount ?? 0} />
          </SimpleGrid>

          {relay?.lastHookType && (
            <Text size="sm">
              最近事件：<Code>{relay.lastTool}</Code> /{" "}
              <Code>{relay.lastHookType}</Code>
              {relay.lastBehavior ? ` → ${relay.lastBehavior}` : " → 释放位置"}
            </Text>
          )}

          {relay?.lastError && (
            <Alert color="red" title="最近一次转发存在失败">
              {relay.lastError}
            </Alert>
          )}
        </Stack>
      </Card>
    </Stack>
  );
}

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
