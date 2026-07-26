import {
  Alert,
  Button,
  Card,
  Group,
  NumberInput,
  Stack,
  Switch,
  Text,
  TextInput,
  Title,
} from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import {
  saveDiscoveryInterval,
  saveMonitorUsername,
  updateAutostart,
} from "../api/monitor";
import {
  monitorKeys,
  monitorSettingsQuery,
  runtimeOverviewQuery,
} from "../queries/monitor";

export function SettingsPage() {
  const queryClient = useQueryClient();
  const runtime = useQuery(runtimeOverviewQuery);
  const settings = useQuery(monitorSettingsQuery);
  const [username, setUsername] = useState("");
  const [discoveryIntervalMinutes, setDiscoveryIntervalMinutes] = useState(1);

  useEffect(() => {
    if (settings.data) setUsername(settings.data.username);
  }, [settings.data]);

  useEffect(() => {
    if (settings.data) {
      setDiscoveryIntervalMinutes(settings.data.discoveryIntervalMinutes);
    }
  }, [settings.data]);

  const autostart = useMutation({
    mutationFn: updateAutostart,
    onSuccess: (data) => queryClient.setQueryData(monitorKeys.runtime(), data),
  });

  const saveUsername = useMutation({
    mutationFn: saveMonitorUsername,
    onSuccess: (data) =>
      queryClient.setQueryData(monitorKeys.settings(), data),
  });

  const saveInterval = useMutation({
    mutationFn: saveDiscoveryInterval,
    onSuccess: (data) =>
      queryClient.setQueryData(monitorKeys.settings(), data),
  });

  const error =
    runtime.error ??
    settings.error ??
    autostart.error ??
    saveUsername.error ??
    saveInterval.error;

  return (
    <Stack gap="md">
      {error && <Alert color="red">{error.message}</Alert>}

      <Card withBorder radius="lg" p="md" className="surface-card">
        <Stack gap="md">
          <div>
            <Title order={3}>通用设置</Title>
            <Text size="sm" c="dimmed" mt={4}>
              显示用户名由所有 AIMonitor 设备共享，不随当前设备切换。
            </Text>
          </div>
          <Group align="flex-end" wrap="nowrap">
            <TextInput
              label="显示用户名"
              description="状态转发时显示在所有设备上的名称"
              placeholder="输入显示用户名"
              value={username}
              onChange={(event) => {
                saveUsername.reset();
                setUsername(event.currentTarget.value);
              }}
              disabled={settings.isPending}
              error={
                !settings.isPending && !username.trim()
                  ? "显示用户名不能为空"
                  : undefined
              }
              style={{ flex: "1 1 auto" }}
            />
            <Button
              onClick={() => saveUsername.mutate(username)}
              loading={saveUsername.isPending}
              disabled={
                !username.trim() || username.trim() === settings.data?.username
              }
            >
              保存用户名
            </Button>
          </Group>
          {saveUsername.isSuccess && (
            <Alert color="teal">显示用户名已保存，并会用于所有设备。</Alert>
          )}

          <Group justify="space-between" align="flex-end">
            <NumberInput
              label="在线设备自动检查间隔"
              description="后台按此间隔重新发现在线设备，默认 1 分钟"
              suffix=" 分钟"
              min={1}
              max={60}
              step={1}
              value={discoveryIntervalMinutes}
              onChange={(value) => {
                saveInterval.reset();
                setDiscoveryIntervalMinutes(
                  typeof value === "number" ? value : 1,
                );
              }}
              disabled={settings.isPending}
              style={{ flex: "1 1 260px" }}
            />
            <Button
              onClick={() => saveInterval.mutate(discoveryIntervalMinutes)}
              loading={saveInterval.isPending}
              disabled={
                !discoveryIntervalMinutes ||
                discoveryIntervalMinutes ===
                  settings.data?.discoveryIntervalMinutes
              }
            >
              保存检查间隔
            </Button>
          </Group>
          {saveInterval.isSuccess && (
            <Alert color="teal">自动检查间隔已保存，立即生效。</Alert>
          )}

          <Switch
            checked={runtime.data?.autostartEnabled ?? false}
            disabled={runtime.isPending}
            label="开机自动运行"
            description="自启时不显示主窗口；再次打开 AIMonitor 会唤起现有窗口。"
            onChange={(event) =>
              autostart.mutate(event.currentTarget.checked)
            }
          />
        </Stack>
      </Card>

    </Stack>
  );
}
