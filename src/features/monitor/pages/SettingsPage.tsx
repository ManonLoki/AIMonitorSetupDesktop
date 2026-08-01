// 引入 Mantine UI 组件：提示、链接、按钮、卡片、分组/堆叠布局、数字输入框、开关、文本、文本输入框、标题
import {
  Alert,
  Anchor,
  Button,
  Card,
  Checkbox,
  Group,
  NumberInput,
  Select,
  SimpleGrid,
  Stack,
  Switch,
  Text,
  TextInput,
  Title,
} from "@mantine/core";
import { openUrl } from "@tauri-apps/plugin-opener";
// 引入 TanStack Query 的 mutation/query hooks 及 queryClient
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
// 引入 React 的副作用与状态 hooks
import { useEffect, useRef, useState } from "react";
import { useSetAtom } from "jotai";
// 引入设置相关的类型化 API 函数
import {
  saveDiscoveryInterval,
  saveEnabledAiTools,
  saveMonitorUsername,
  updateAutostart,
} from "../api/monitor";
import type { AiTool } from "../api/monitor";
import { HooksManagementCard } from "../components/HooksManagementCard";
// 引入查询键与预定义的查询配置
import {
  monitorKeys,
  monitorCapabilitiesQuery,
  monitorSettingsQuery,
  runtimeOverviewQuery,
} from "../queries/monitor";
import {
  onboardingCompletedAtom,
  onboardingOpenAtom,
} from "../../../shared/state/ui";
import { LineIcon } from "../../../shared/ui/LineIcon";
import { useI18n } from "../../../shared/i18n";
import type { LanguagePreference } from "../../../shared/state/ui";

const AUTHOR = "ManonLoki";
const GITHUB_URL = "https://github.com/ManonLoki/AIMonitorSetupDesktop";

export function SettingsPage() {
  const { t, preference, setPreference } = useI18n();
  // 获取 QueryClient 实例，用于在 mutation 成功后手动写入缓存
  const queryClient = useQueryClient();
  const setOnboardingCompleted = useSetAtom(onboardingCompletedAtom);
  const setOnboardingOpen = useSetAtom(onboardingOpenAtom);
  // 查询运行时概览信息（自启动状态等），对接 get_runtime_overview 命令，每 3 秒自动轮询一次
  const runtime = useQuery(runtimeOverviewQuery);
  // 查询当前监控设置（用户名、发现间隔等），对接 get_monitor_settings 命令，视为永久新鲜（仅手动失效）
  const settings = useQuery(monitorSettingsQuery);
  // Rust 领域层声明的工具目录与数值范围，应用运行期间保持不变。
  const capabilities = useQuery(monitorCapabilitiesQuery);
  // 显示用户名的本地编辑草稿
  const [username, setUsername] = useState("");
  // 在线设备自动检查间隔（分钟）的本地编辑草稿，默认 1 分钟
  const [discoveryIntervalMinutes, setDiscoveryIntervalMinutes] = useState<
    number | string
  >("");
  // 设置页 AI 客户端多选草稿；保存后同时影响监控管理与 Hooks 管理的选项卡。
  const [selectedTools, setSelectedTools] = useState<AiTool[]>([]);
  const draftsInitialized = useRef(false);

  // 只初始化一次；后续保存某一字段时不能覆盖其他尚未提交的本地草稿。
  useEffect(() => {
    if (!settings.data || draftsInitialized.current) return;
    draftsInitialized.current = true;
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
    onSuccess: (data) => {
      queryClient.setQueryData(monitorKeys.settings(), data);
      setUsername(data.username);
    },
  });

  // 保存自动检查间隔的 mutation：对接 save_discovery_interval 命令；成功后把返回的最新设置写入 settings 查询缓存
  const saveInterval = useMutation({
    mutationFn: saveDiscoveryInterval,
    onSuccess: (data) => {
      queryClient.setQueryData(monitorKeys.settings(), data);
      setDiscoveryIntervalMinutes(data.discoveryIntervalMinutes);
    },
  });

  // 保存勾选的 AI 客户端的 mutation：对接 save_enabled_ai_tools 命令；成功后把返回的最新设置写入 settings 查询缓存
  const saveTools = useMutation({
    mutationFn: saveEnabledAiTools,
    onSuccess: (data) => {
      queryClient.setQueryData(monitorKeys.settings(), data);
      setSelectedTools(data.enabledAiTools);
    },
  });

  // 汇总所有相关查询/mutation 的错误，任意一个出错就展示错误提示
  const error =
    runtime.error ??
    settings.error ??
    capabilities.error ??
    autostart.error ??
    saveUsername.error ??
    saveInterval.error ??
    saveTools.error;

  // 已保存的 AI 客户端列表，同时用作 HooksManagementCard 的可见范围
  const savedTools = settings.data?.enabledAiTools ?? [];
  const aiTools = capabilities.data?.aiTools ?? [];
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
              <Title order={4}>{t("settings.aiClients")}</Title>
              <Text size="xs" c="dimmed" mt={2}>
                {t("settings.aiClientsDescription")}
              </Text>
            </div>
            <Group gap="xs" wrap="nowrap">
              {saveTools.isSuccess && (
                <Text size="xs" c="teal" style={{ whiteSpace: "nowrap" }}>
                  {t("common.saved")}
                </Text>
              )}
              <Button
                size="xs"
                onClick={() => saveTools.mutate(selectedTools)}
                loading={saveTools.isPending}
                disabled={!toolsDirty}
              >
                {t("settings.saveAiClients")}
              </Button>
            </Group>
          </Group>
          <SimpleGrid
            cols={{ base: 2, sm: 3, md: 4 }}
            spacing="xs"
            verticalSpacing={4}
          >
            {aiTools.map((tool) => (
              <Checkbox
                key={tool.tool}
                label={tool.name}
                size="sm"
                checked={selectedTools.includes(tool.tool)}
                disabled={
                  settings.isPending ||
                  capabilities.isPending ||
                  saveTools.isPending
                }
                onChange={(event) => {
                  const checked = event.currentTarget.checked;
                  saveTools.reset();
                  setSelectedTools((current) =>
                    checked
                      ? [...current, tool.tool]
                      : current.filter((value) => value !== tool.tool),
                  );
                }}
              />
            ))}
          </SimpleGrid>
        </Stack>
      </Card>

      {/* Hooks 配置目录与写入管理卡片，使用已保存（非草稿）的可见工具范围 */}
      <HooksManagementCard enabledTools={savedTools} aiTools={aiTools} />

      <Card
        withBorder
        radius="lg"
        p="sm"
        className="surface-card settings-card"
      >
        <Stack gap="sm">
          {/* 通用设置标题与说明文案 */}
          <Group justify="space-between" align="flex-start" wrap="wrap">
            <div>
              <Title order={4}>{t("settings.general")}</Title>
              <Text size="xs" c="dimmed" mt={2}>
                {t("settings.generalDescription")}
              </Text>
            </div>
            <Button
              size="xs"
              variant="default"
              leftSection={<LineIcon name="guide" size={16} />}
              onClick={() => {
                setOnboardingCompleted(false);
                setOnboardingOpen(true);
              }}
            >
              {t("settings.onboarding")}
            </Button>
          </Group>
          <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="md">
            {/* 显示用户名输入框与保存按钮 */}
            <Group align="flex-end" wrap="nowrap">
              <TextInput
                size="xs"
                label={t("settings.username")}
                description={t("settings.usernameDescription")}
                placeholder={t("settings.usernamePlaceholder")}
                value={username}
                onChange={(event) => {
                  // 编辑用户名时清空上一次保存结果状态
                  saveUsername.reset();
                  setUsername(event.currentTarget.value);
                }}
                disabled={settings.isPending}
                error={
                  !settings.isPending && !username.trim()
                    ? t("settings.usernameRequired")
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
                {t("settings.saveUsername")}
              </Button>
            </Group>

            {/* 在线设备自动检查间隔输入框与保存按钮 */}
            <Group align="flex-end" wrap="nowrap">
              <NumberInput
                size="xs"
                label={t("settings.interval")}
                description={t("settings.intervalDescription")}
                suffix={t("settings.minutesSuffix")}
                min={capabilities.data?.discoveryInterval.min}
                max={capabilities.data?.discoveryInterval.max}
                step={1}
                value={discoveryIntervalMinutes}
                onChange={(value) => {
                  // 编辑间隔时清空上一次保存结果状态；范围由 Rust capability 提供。
                  saveInterval.reset();
                  setDiscoveryIntervalMinutes(value);
                }}
                disabled={settings.isPending}
                style={{ flex: "1 1 auto", minWidth: 0 }}
              />
              <Button
                size="xs"
                // 保存当前编辑的自动检查间隔
                onClick={() => {
                  if (typeof discoveryIntervalMinutes === "number") {
                    saveInterval.mutate(discoveryIntervalMinutes);
                  }
                }}
                loading={saveInterval.isPending}
                disabled={
                  typeof discoveryIntervalMinutes !== "number" ||
                  discoveryIntervalMinutes ===
                    settings.data?.discoveryIntervalMinutes
                }
              >
                {t("settings.saveInterval")}
              </Button>
            </Group>
          </SimpleGrid>
          {(saveUsername.isSuccess || saveInterval.isSuccess) && (
            <Group gap="md">
              {saveUsername.isSuccess && (
                <Text size="xs" c="teal">
                  {t("settings.usernameSaved")}
                </Text>
              )}
              {saveInterval.isSuccess && (
                <Text size="xs" c="teal">
                  {t("settings.intervalSaved")}
                </Text>
              )}
            </Group>
          )}

          <Select
            size="xs"
            label={t("language.label")}
            description={t("language.description")}
            value={preference}
            data={[
              { value: "system", label: t("language.system") },
              { value: "zh-CN", label: t("language.zhCN") },
              { value: "en", label: t("language.en") },
            ]}
            onChange={(value) => {
              if (value) setPreference(value as LanguagePreference);
            }}
          />

          {/* 开机自动运行开关，切换时直接触发 mutation */}
          <Switch
            size="sm"
            checked={runtime.data?.autostartEnabled ?? false}
            disabled={runtime.isPending}
            label={t("settings.autostart")}
            description={t("settings.autostartDescription")}
            onChange={(event) =>
              autostart.mutate(event.currentTarget.checked)
            }
          />
        </Stack>
      </Card>

      <Card
        withBorder
        radius="lg"
        p="sm"
        className="surface-card settings-card"
      >
        <Stack gap="xs">
          <div>
            <Title order={4}>{t("settings.about")}</Title>
            <Text size="xs" c="dimmed" mt={2}>
              {t("settings.aboutDescription")}
            </Text>
          </div>
          <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="md">
            <div>
              <Text size="xs" c="dimmed">
                {t("settings.author")}
              </Text>
              <Text size="sm" fw={600}>
                {AUTHOR}
              </Text>
            </div>
            <div>
              <Text size="xs" c="dimmed">
                GitHub
              </Text>
              <Anchor
                href={GITHUB_URL}
                size="sm"
                fw={600}
                onClick={(event) => {
                  event.preventDefault();
                  void openUrl(GITHUB_URL);
                }}
              >
                {GITHUB_URL}
              </Anchor>
            </div>
          </SimpleGrid>
        </Stack>
      </Card>
    </Stack>
  );
}
