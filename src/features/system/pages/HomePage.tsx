import {
  Alert,
  Badge,
  Card,
  Group,
  Loader,
  SimpleGrid,
  Stack,
  Text,
  Title,
} from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { systemOverviewQuery } from "../queries/system";

export function HomePage() {
  const overview = useQuery(systemOverviewQuery);

  if (overview.isPending) {
    return (
      <Group justify="center" py="xl">
        <Loader aria-label="正在连接 Rust 后端" />
      </Group>
    );
  }

  if (overview.isError) {
    return (
      <Alert color="red" title="无法连接 Rust 后端">
        {overview.error.message}
      </Alert>
    );
  }

  return (
    <Stack gap="xl">
      <Stack gap="xs">
        <Badge variant="light" w="fit-content">
          Tauri desktop foundation
        </Badge>
        <Title order={1}>AI Monitor</Title>
        <Text c="dimmed" maw={680}>
          前端负责界面、交互状态与后端调用，业务规则和数据处理全部由 Rust
          承担。
        </Text>
      </Stack>

      <SimpleGrid cols={{ base: 1, sm: 2 }}>
        <InfoCard label="应用" value={overview.data.appName} />
        <InfoCard label="版本" value={overview.data.version} />
        <InfoCard label="业务后端" value={overview.data.backend} />
        <InfoCard label="调用通道" value={overview.data.transport} />
      </SimpleGrid>
    </Stack>
  );
}

function InfoCard({ label, value }: { label: string; value: string }) {
  return (
    <Card withBorder>
      <Text c="dimmed" size="sm">
        {label}
      </Text>
      <Text fw={600} size="lg">
        {value}
      </Text>
    </Card>
  );
}
