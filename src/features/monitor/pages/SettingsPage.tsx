// 引入 Mantine UI 组件：提示、按钮、卡片、分组/堆叠布局、数字输入框、开关、文本、文本输入框、标题
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
// 引入 TanStack Query 的 mutation/query hooks 及 queryClient
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
// 引入 React 的副作用与状态 hooks
import { useEffect, useState } from "react";
// 引入设置相关的类型化 API 函数
import {
  saveDiscoveryInterval,
  saveMonitorUsername,
  updateAutostart,
} from "../api/monitor";
// 引入查询键与预定义的查询配置
import {
  monitorKeys,
  monitorSettingsQuery,
  runtimeOverviewQuery,
} from "../queries/monitor";

export function SettingsPage() {
  // 获取 QueryClient 实例，用于在 mutation 成功后手动写入缓存
  const queryClient = useQueryClient();
  // 查询运行时概览信息（自启动状态等），对接 get_runtime_overview 命令，每 3 秒自动轮询一次
  const runtime = useQuery(runtimeOverviewQuery);
  // 查询当前监控设置（用户名、发现间隔等），对接 get_monitor_settings 命令，视为永久新鲜（仅手动失效）
  const settings = useQuery(monitorSettingsQuery);
  // 显示用户名的本地编辑草稿
  const [username, setUsername] = useState("");
  // 在线设备自动检查间隔（分钟）的本地编辑草稿，默认 1 分钟
  const [discoveryIntervalMinutes, setDiscoveryIntervalMinutes] = useState(1);

  // 当远端设置数据到达后，用远端数据初始化本地用户名与检查间隔草稿
  useEffect(() => {
    if (!settings.data) return;
    setUsername(settings.data.username);
    setDiscoveryIntervalMinutes(settings.data.discoveryIntervalMinutes);
  }, [settings.data]);

  // 更新开机自启开关的 mutation：对接 update_autostart 命令；成功后直接把返回的运行时概览写入 runtime 查询缓存
  const autostart = useMutation({
    mutationFn: updateAutostart,
    onSuccess: (data) => queryClient.setQueryData(monitorKeys.runtime(), data),
  });

  // 保存显示用户名的 mutation：对接 save_monitor_username 命令；成功后把返回的最新设置写入 settings 查询缓存
  const saveUsername = useMutation({
    mutationFn: saveMonitorUsername,
    onSuccess: (data) =>
      queryClient.setQueryData(monitorKeys.settings(), data),
  });

  // 保存自动检查间隔的 mutation：对接 save_discovery_interval 命令；成功后把返回的最新设置写入 settings 查询缓存
  const saveInterval = useMutation({
    mutationFn: saveDiscoveryInterval,
    onSuccess: (data) =>
      queryClient.setQueryData(monitorKeys.settings(), data),
  });

  // 汇总所有相关查询/mutation 的错误，任意一个出错就展示错误提示
  const error =
    runtime.error ??
    settings.error ??
    autostart.error ??
    saveUsername.error ??
    saveInterval.error;

  return (
    <Stack gap="md">
      {/* 汇总错误提示 */}
      {error && <Alert color="red">{error.message}</Alert>}

      <Card withBorder radius="lg" p="md" className="surface-card">
        <Stack gap="md">
          {/* 通用设置标题与说明文案 */}
          <div>
            <Title order={3}>通用设置</Title>
            <Text size="sm" c="dimmed" mt={4}>
              显示用户名由所有 AIMonitor 设备共享，不随当前设备切换。
            </Text>
          </div>
          {/* 显示用户名输入框与保存按钮 */}
          <Group align="flex-end" wrap="nowrap">
            <TextInput
              label="显示用户名"
              description="状态转发时显示在所有设备上的名称"
              placeholder="输入显示用户名"
              value={username}
              onChange={(event) => {
                // 编辑用户名时清空上一次保存结果状态
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
              // 保存当前编辑的显示用户名
              onClick={() => saveUsername.mutate(username)}
              loading={saveUsername.isPending}
              disabled={
                !username.trim() || username.trim() === settings.data?.username
              }
            >
              保存用户名
            </Button>
          </Group>
          {/* 用户名保存成功后的提示 */}
          {saveUsername.isSuccess && (
            <Alert color="teal">显示用户名已保存，并会用于所有设备。</Alert>
          )}

          {/* 在线设备自动检查间隔输入框与保存按钮 */}
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
                // 编辑间隔时清空上一次保存结果状态；非数字输入时兜底为 1 分钟
                saveInterval.reset();
                setDiscoveryIntervalMinutes(
                  typeof value === "number" ? value : 1,
                );
              }}
              disabled={settings.isPending}
              style={{ flex: "1 1 260px" }}
            />
            <Button
              // 保存当前编辑的自动检查间隔
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
          {/* 检查间隔保存成功后的提示 */}
          {saveInterval.isSuccess && (
            <Alert color="teal">自动检查间隔已保存，立即生效。</Alert>
          )}

          {/* 开机自动运行开关，切换时直接触发 mutation */}
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
