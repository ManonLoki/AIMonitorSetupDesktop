import { Card, Code, List, Stack, Text, Title } from "@mantine/core";

const layers = [
  "Mantine：视觉组件与主题",
  "TanStack Router：路由和页面边界",
  "TanStack Query：Rust command 调用的异步生命周期与缓存",
  "Jotai：仅保存短生命周期的前端 UI 状态",
  "Tauri invoke：前端与 Rust 的唯一默认通信边界",
];

export function ArchitecturePage() {
  return (
    <Stack gap="lg">
      <div>
        <Title order={1}>架构边界</Title>
        <Text c="dimmed">
          UI 保持薄层；任何可测试的业务判断、聚合、校验与持久化都放在 Rust。
        </Text>
      </div>

      <Card withBorder>
        <List spacing="sm">
          {layers.map((layer) => (
            <List.Item key={layer}>{layer}</List.Item>
          ))}
        </List>
      </Card>

      <Text>
        新增调用时遵循：
        <Code>Rust domain → Tauri command → typed API → Query hook → page</Code>
      </Text>
    </Stack>
  );
}
