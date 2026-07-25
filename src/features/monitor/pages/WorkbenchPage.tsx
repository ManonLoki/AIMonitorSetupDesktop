import {
  Alert,
  Badge,
  Card,
  Code,
  Group,
  SimpleGrid,
  Stack,
  Text,
  Title,
} from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { runtimeOverviewQuery } from "../queries/monitor";

export function WorkbenchPage() {
  const runtime = useQuery(runtimeOverviewQuery);
  const relay = runtime.data?.hookRelay;

  return (
    <Stack gap="lg">
      <div>
        <Title order={2}>工作台</Title>
        <Text size="sm" c="dimmed" mt={4}>
          查看本机 Hook 中继状态及多设备转发结果。
        </Text>
      </div>

      {runtime.error && <Alert color="red">{runtime.error.message}</Alert>}

      <Card withBorder radius="lg" p="xl" className="surface-card">
        <Stack gap="lg">
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

          <div className="endpoint-preview">
            <Text size="xs" c="dimmed" mb={6}>
              本机入口
            </Text>
            <Code>
              http://{relay?.bindAddress ?? "127.0.0.1"}:
              {relay?.port ?? 10240}/api/hooks/{"{tool}"}
            </Code>
            <Text size="xs" c="dimmed" mt={8}>
              请求体仅包含 <Code>{`{"type":"HookType"}`}</Code>，AI
              标识由 URL 路径提供。
            </Text>
          </div>

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
