// 引入 Mantine UI 组件：Alert 提示、Badge 徽标、Button 按钮、Card 卡片、分组/堆叠布局、栅格、Tabs 选项卡与文本控件
import {
  Alert,
  Badge,
  Button,
  Card,
  Group,
  SimpleGrid,
  Stack,
  Tabs,
  Text,
  Textarea,
} from "@mantine/core";
// 引入 TanStack Query 的 mutation/query hooks 及 queryClient，用于对接 Rust 后端命令
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
// 引入 React 的副作用、记忆化和状态 hooks
import { useEffect, useMemo, useState } from "react";
// 引入通用图标组件
import { LineIcon } from "../../../shared/ui/LineIcon";
// 引入本 feature 的类型定义
import type { AiProfile, AiTool, HookBehavior } from "../api/monitor";
// 引入本 feature 对接 Rust 命令的类型化 API 函数
import { saveAiProfile } from "../api/monitor";
// 引入全部受支持 AI 工具的取值/展示名映射与可见性过滤函数
import { AI_TOOLS, enabledAiTools } from "../components/aiTools";
// 引入图片选择器组件
import { ImagePicker } from "../components/ImagePicker";
// 引入设备未就绪时的统一拦截组件
import { useMonitorDeviceGate } from "../components/MonitorDeviceGate";
// 引入展示位选择器组件
import { SlotPicker } from "../components/SlotPicker";
// 引入维护当前工具 Tab、自动回退到可见工具的共享 hook
import { useActiveVisibleTool } from "../hooks/useActiveVisibleTool";
// 引入监控设备连接状态 hook
import { useMonitorConnection } from "../hooks/useMonitorConnection";
import { useDirectImageUpload } from "../hooks/useDirectImageUpload";
// 引入查询键与预定义的查询配置
import {
  aiProfilesQuery,
  monitorKeys,
  remoteImagesQuery,
} from "../queries/monitor";
import { useI18n } from "../../../shared/i18n";

// 可配置的 hook 行为列表：取值、展示名与徽标颜色，用于渲染各行为的展示配置卡片
const behaviors: Array<{
  value: HookBehavior;
  color: string;
}> = [
  { value: "idle", color: "gray" },
  { value: "running", color: "violet" },
  { value: "asking", color: "yellow" },
  { value: "error", color: "red" },
];

// 构造某个工具的空白 Profile 草稿：deviceId 留空、slot 默认第 1 位，每种行为都生成一条空 hook
function emptyProfile(tool: AiTool): AiProfile {
  return {
    deviceId: "",
    tool,
    slot: 1,
    hooks: behaviors.map(({ value }) => ({
      behavior: value,
      content: "",
      image: "",
    })),
  };
}

// 按 AI_TOOLS 固定顺序为全部工具生成空白草稿，避免逐个手写全部取值
function initialDrafts(): Record<AiTool, AiProfile> {
  return Object.fromEntries(
    AI_TOOLS.map(({ value }) => [value, emptyProfile(value)]),
  ) as Record<AiTool, AiProfile>;
}

export function AiManagementPage() {
  const { t } = useI18n();
  // 获取 QueryClient 实例，用于在 mutation 成功后手动写入缓存
  const queryClient = useQueryClient();
  // 查询当前设备下所有 AI 工具的 Profile 列表（对接 list_ai_profiles 命令）
  const profiles = useQuery(aiProfilesQuery);
  // 获取当前监控设备的连接状态：设置、设备列表、是否已配置/可用设备、是否处于加载中
  const {
    settings: monitorSettings,
    devices,
    hasConfiguredDevice,
    hasAvailableDevice,
    isPending: monitorPending,
  } = useMonitorConnection();
  // 查询远端图片列表（对接 list_remote_images 命令），仅在设备已配置且可用时才发起请求
  const images = useQuery({
    ...remoteImagesQuery,
    enabled: hasConfiguredDevice && hasAvailableDevice,
  });
  // 设置页勾选的 AI 客户端列表；未加载完成前视为空，避免闪现全部工具
  const enabledTools = monitorSettings.data?.enabledAiTools ?? [];
  // 按固定顺序过滤出当前可见的工具，仅在勾选集合变化时重新计算
  const visibleTools = useMemo(
    () => enabledAiTools(enabledTools),
    [enabledTools],
  );
  // 当前选中的 AI 工具 Tab；已选工具被取消勾选后自动回退到第一个可见工具
  const [activeTool, setActiveTool] = useActiveVisibleTool(visibleTools);
  // 各工具的展示配置草稿（未保存前的编辑态数据）
  const [drafts, setDrafts] =
    useState<Record<AiTool, AiProfile>>(initialDrafts);
  const draft = drafts[activeTool];
  // 当前工具已选择图片的行为数量
  const configuredBehaviorCount = draft.hooks.filter(
    (hook) => hook.image.length > 0,
  ).length;
  // 是否所有行为都已配置图片，决定“保存展示配置”按钮是否可用
  const isComplete =
    draft.hooks.length === behaviors.length &&
    configuredBehaviorCount === draft.hooks.length;

  // 当远端 Profile 数据到达后，用远端数据覆盖本地草稿（未保存过的工具保留空白草稿）
  useEffect(() => {
    if (!profiles.data) return;
    setDrafts(() => {
      const next = initialDrafts();
      for (const profile of profiles.data) next[profile.tool] = profile;
      return next;
    });
  }, [profiles.data]);

  // 保存展示配置 Profile 的 mutation：对接 save_ai_profile 命令；成功后直接把返回结果合并进 profiles 查询缓存，替换同工具的旧记录
  const save = useMutation({
    mutationFn: saveAiProfile,
    onSuccess: (savedProfile) => {
      queryClient.setQueryData<AiProfile[]>(
        monitorKeys.profiles(),
        (current = []) => [
          ...current.filter((profile) => profile.tool !== savedProfile.tool),
          savedProfile,
        ],
      );
    },
  });

  const { upload, uploadedSelection, clearUploadedSelection } =
    useDirectImageUpload({
      setDrafts,
      onDraftChange: save.reset,
    });

  // 更新当前激活工具的草稿：先清空 save 的结果状态，再写入新的草稿内容
  const updateDraft = (next: AiProfile) => {
    save.reset();
    setDrafts((current) => ({ ...current, [activeTool]: next }));
  };

  // 更新某个行为对应 hook 的单个字段（图片或文本内容）
  const updateHookField = (
    behaviorValue: HookBehavior,
    field: "image" | "content",
    value: string,
  ) => {
    updateDraft({
      ...draft,
      hooks: draft.hooks.map((item) =>
        item.behavior === behaviorValue ? { ...item, [field]: value } : item,
      ),
    });
  };

  // 汇总所有相关查询/mutation 的错误，任意一个出错就展示错误提示
  const error =
    profiles.error ??
    devices.error ??
    monitorSettings.error ??
    images.error ??
    upload.error ??
    save.error;
  // 可供选择的远端图片列表，查询未返回数据时兜底为空数组
  const availableImages = images.data ?? [];
  // 设备门禁：设备未配置或不可用时，返回统一的拦截提示，阻止渲染正式页面内容
  const deviceGate = useMonitorDeviceGate({
    isPending: monitorPending,
    hasConfiguredDevice,
    hasAvailableDevice,
    featureLabel: t("monitor.feature"),
  });
  if (deviceGate) return deviceGate;

  if (visibleTools.length === 0) {
    return (
      <Alert color="blue">
        {t("monitor.noClient")}
      </Alert>
    );
  }

  return (
    <Stack gap="md">
      {error && <Alert color="red">{error.message}</Alert>}
      {uploadedSelection?.tool === activeTool &&
        uploadedSelection.deviceId === monitorSettings.data?.deviceId && (
          <Alert color="teal" variant="light">
            {t("image.uploadedAndSelected", {
              filename: uploadedSelection.filename,
            })}
          </Alert>
        )}
      <Tabs
        value={activeTool}
        onChange={(value) => {
          if (!value) return;
          save.reset();
          setActiveTool(value as AiTool);
        }}
        className="ai-tool-tabs"
      >
        <Tabs.List grow>
          {visibleTools.map((tool) => (
            <Tabs.Tab key={tool.value} value={tool.value}>
              {tool.label}
            </Tabs.Tab>
          ))}
        </Tabs.List>

        {visibleTools.map((tool) => (
          <Tabs.Panel key={tool.value} value={tool.value} pt="md">
            <Stack gap="lg">
              <Card
                withBorder
                className="surface-card slot-section-card"
                p="lg"
              >
                <SlotPicker
                  value={draft.slot}
                  onChange={(slot) => updateDraft({ ...draft, slot })}
                />
              </Card>

              <Group justify="space-between" align="flex-end">
                <div>
                  <Text fw={650}>{t("monitor.behaviorDisplay")}</Text>
                  <Text size="sm" c="dimmed" mt={3}>
                    {t("monitor.behaviorDescription")}
                  </Text>
                </div>
                <Badge variant="light" color="violet" size="lg">
                  {t("monitor.configuredCount", { configured: configuredBehaviorCount, total: behaviors.length })}
                </Badge>
              </Group>

              {!images.isPending && availableImages.length === 0 && (
                <Alert color="yellow" variant="light">
                  {t("monitor.noImages")}
                </Alert>
              )}

              <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="lg">
                {behaviors.map((behavior) => {
                  const hook = draft.hooks.find(
                    (item) => item.behavior === behavior.value,
                  ) ?? {
                    behavior: behavior.value,
                    content: "",
                    image: "",
                  };
                  return (
                    <Card
                      key={behavior.value}
                      withBorder
                      className="behavior-card"
                      data-behavior={behavior.value}
                      p={0}
                    >
                      <div className="behavior-card-header">
                        <Group justify="space-between" wrap="nowrap">
                          <Group gap="sm" wrap="nowrap">
                            <span className="behavior-card-status" />
                            <div>
                              <Text fw={700}>{t(`behavior.${behavior.value}`)}</Text>
                              <Text size="xs" c="dimmed" mt={1}>
                                {t("monitor.stateDescription")}
                              </Text>
                            </div>
                          </Group>
                          <Badge
                            color={hook.image ? "teal" : behavior.color}
                            variant="light"
                            radius="sm"
                          >
                            {hook.image ? t("common.configured") : t("common.notConfigured")}
                          </Badge>
                        </Group>
                      </div>
                      <Stack gap="md" className="behavior-card-content">
                        <ImagePicker
                          images={availableImages}
                          value={hook.image}
                          disabled={images.isPending || upload.isPending}
                          uploading={
                            upload.isPending &&
                            upload.variables?.tool === tool.value &&
                            upload.variables?.behavior === behavior.value
                          }
                          onChange={(value) => {
                            clearUploadedSelection();
                            updateHookField(behavior.value, "image", value);
                          }}
                          onUpload={(file) => {
                            const deviceId = monitorSettings.data?.deviceId;
                            if (!deviceId) return;
                            upload.mutate({
                              file,
                              tool: tool.value,
                              behavior: behavior.value,
                              deviceId,
                            });
                          }}
                        />
                        <Textarea
                          label={
                            <span>
                              {t("monitor.content")}{" "}
                              <Text
                                component="span"
                                size="xs"
                                fw={400}
                                c="dimmed"
                              >
                                {t("common.optional")}
                              </Text>
                            </span>
                          }
                          placeholder={t("monitor.contentPlaceholder")}
                          autosize
                          minRows={1}
                          value={hook.content}
                          onChange={(event) =>
                            updateHookField(
                              behavior.value,
                              "content",
                              event.currentTarget.value,
                            )
                          }
                        />
                      </Stack>
                    </Card>
                  );
                })}
              </SimpleGrid>

              <Card withBorder className="profile-save-bar" p="sm">
                <Group justify="space-between" wrap="wrap">
                  <div>
                    <Text size="sm" fw={650}>
                      {t("monitor.displayConfig")}
                    </Text>
                    <Text size="xs" c="dimmed" mt={2}>
                      {isComplete
                        ? t("monitor.ready")
                        : t("monitor.remaining", { count: behaviors.length - configuredBehaviorCount })}
                    </Text>
                  </div>
                  <Group>
                    {save.isSuccess && save.data.tool === activeTool && (
                      <Badge variant="light" color="teal">
                        {t("common.saved")}
                      </Badge>
                    )}
                    <Button
                      leftSection={<LineIcon name="check" size={17} />}
                      onClick={() => save.mutate(draft)}
                      loading={save.isPending}
                      disabled={!isComplete}
                    >
                      {t("monitor.save")}
                    </Button>
                  </Group>
                </Group>
              </Card>
            </Stack>
          </Tabs.Panel>
        ))}
      </Tabs>
    </Stack>
  );
}
