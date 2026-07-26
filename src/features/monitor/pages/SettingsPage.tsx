// 引入 Mantine UI 组件：提示、按钮、卡片、分组/堆叠布局、数字输入框、开关、文本、文本输入框、标题
import {
  Alert,
  Button,
  Card,
  Checkbox,
  Group,
  NumberInput,
  SimpleGrid,
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
  saveEnabledAiTools,
  saveMonitorUsername,
  updateAutostart,
} from "../api/monitor";
import type { AiTool } from "../api/monitor";
import { AI_TOOLS } from "../components/aiTools";
import { HooksManagementCard } from "../components/HooksManagementCard";
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
  // 设置页 AI 客户端多选草稿；保存后同时影响监控管理与 Hooks 管理的选项卡。
  const [selectedTools, setSelectedTools] = useState<AiTool[]>([]);

  // 当远端设置数据到达后，用远端数据初始化本地用户名与检查间隔草稿
  useEffect(() => {
    if (!settings.data) return;
    setUsername(settings.data.username);
    setDiscoveryIntervalMinutes(settings.data.discoveryIntervalMinutes);
    setSelectedTools(settings.data.enabledAiTools);
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

  // 保存勾选的 AI 客户端的 mutation：对接 save_enabled_ai_tools 命令；成功后把返回的最新设置写入 settings 查询缓存
  const saveTools = useMutation({
    mutationFn: saveEnabledAiTools,
    onSuccess: (data) =>
      queryClient.setQueryData(monitorKeys.settings(), data),
  });

  // 汇总所有相关查询/mutation 的错误，任意一个出错就展示错误提示
  const error =
    runtime.error ??
    settings.error ??
    autostart.error ??
    saveUsername.error ??
    saveInterval.error ??
    saveTools.error;

  // 已保存的 AI 客户端列表，同时用作 HooksManagementCard 的可见范围
  const savedTools = settings.data?.enabledAiTools ?? [];
  // 本地勾选草稿是否与已保存列表不同，决定“保存 AI 客户端”按钮是否可用
  const toolsDirty =
    selectedTools.length !== savedTools.length ||
    selectedTools.some((tool) => !savedTools.includes(tool));

  return (
    <Stack gap="sm" className="settings-page">
      {/* 汇总错误提示 */}
      {error && <Alert color="red">{error.message}</Alert>}

      {/* AI 客户端多选卡片：勾选后同步调整监控管理与 Hooks 管理的可见选项卡 */}
      <Card
        withBorder
        radius="lg"
        p="sm"
        className="surface-card settings-card"
      >
        <Stack gap="sm">
          <Group justify="space-between" align="flex-start" wrap="nowrap">
            <div>
              <Title order={4}>AI 客户端</Title>
              <Text size="xs" c="dimmed" mt={2}>
                选择需要管理的客户端；保存后会同步调整监控管理和 Hooks 管理中的选项卡。
              </Text>
            </div>
            <Group gap="xs" wrap="nowrap">
              {saveTools.isSuccess && (
                <Text size="xs" c="teal" style={{ whiteSpace: "nowrap" }}>
                  已保存
                </Text>
              )}
              <Button
                size="xs"
                onClick={() => saveTools.mutate(selectedTools)}
                loading={saveTools.isPending}
                disabled={!toolsDirty}
              >
                保存 AI 客户端
              </Button>
            </Group>
          </Group>
          <SimpleGrid
            cols={{ base: 2, sm: 3, md: 4 }}
            spacing="xs"
            verticalSpacing={4}
          >
            {AI_TOOLS.map((tool) => (
              <Checkbox
                key={tool.value}
                label={tool.label}
                size="sm"
                checked={selectedTools.includes(tool.value)}
                disabled={settings.isPending || saveTools.isPending}
                onChange={(event) => {
                  const checked = event.currentTarget.checked;
                  saveTools.reset();
                  setSelectedTools((current) =>
                    checked
                      ? [...current, tool.value]
                      : current.filter((value) => value !== tool.value),
                  );
                }}
              />
            ))}
          </SimpleGrid>
        </Stack>
      </Card>

      {/* Hooks 配置目录与写入管理卡片，使用已保存（非草稿）的可见工具范围 */}
      <HooksManagementCard enabledTools={savedTools} />

      <Card
        withBorder
        radius="lg"
        p="sm"
        className="surface-card settings-card"
      >
        <Stack gap="sm">
          {/* 通用设置标题与说明文案 */}
          <div>
            <Title order={4}>通用设置</Title>
            <Text size="xs" c="dimmed" mt={2}>
              显示用户名由所有 AIMonitor 设备共享，不随当前设备切换。
            </Text>
          </div>
          <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="md">
            {/* 显示用户名输入框与保存按钮 */}
            <Group align="flex-end" wrap="nowrap">
              <TextInput
                size="xs"
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
                style={{ flex: "1 1 auto", minWidth: 0 }}
              />
              <Button
                size="xs"
                // 保存当前编辑的显示用户名
                onClick={() => saveUsername.mutate(username)}
                loading={saveUsername.isPending}
                disabled={
                  !username.trim() ||
                  username.trim() === settings.data?.username
                }
              >
                保存用户名
              </Button>
            </Group>

            {/* 在线设备自动检查间隔输入框与保存按钮 */}
            <Group align="flex-end" wrap="nowrap">
              <NumberInput
                size="xs"
                label="在线设备自动检查间隔"
                description="后台重新发现在线设备，默认 1 分钟"
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
                style={{ flex: "1 1 auto", minWidth: 0 }}
              />
              <Button
                size="xs"
                // 保存当前编辑的自动检查间隔
                onClick={() => saveInterval.mutate(discoveryIntervalMinutes)}
                loading={saveInterval.isPending}
                disabled={
                  !discoveryIntervalMinutes ||
                  discoveryIntervalMinutes ===
                    settings.data?.discoveryIntervalMinutes
                }
              >
                保存间隔
              </Button>
            </Group>
          </SimpleGrid>
          {(saveUsername.isSuccess || saveInterval.isSuccess) && (
            <Group gap="md">
              {saveUsername.isSuccess && (
                <Text size="xs" c="teal">
                  显示用户名已保存。
                </Text>
              )}
              {saveInterval.isSuccess && (
                <Text size="xs" c="teal">
                  自动检查间隔已保存，立即生效。
                </Text>
              )}
            </Group>
          )}

          {/* 开机自动运行开关，切换时直接触发 mutation */}
          <Switch
            size="sm"
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
