// 引入 Mantine UI 组件：Alert 提示、Badge 徽标、Button 按钮、Card 卡片、分组/堆叠布局、栅格、Tabs 选项卡与文本控件
import {
  Alert,
  Badge,
  Button,
  Card,
  Group,
  Loader,
  SimpleGrid,
  Stack,
  Tabs,
  Text,
  Textarea,
} from "@mantine/core";
// 引入 TanStack Query 的 query hook，用于读取 Rust 后端状态
import { useQuery } from "@tanstack/react-query";
// 引入 React 的副作用、记忆化和状态 hooks
import { useEffect, useMemo, useState } from "react";
// 引入通用图标组件
import { LineIcon } from "../../../shared/ui/LineIcon";
// 引入本 feature 的类型定义
import type { AiProfileDraft, AiTool, HookBehavior } from "../api/monitor";
// 引入全部受支持 AI 工具的取值/展示名映射与可见性过滤函数
import { enabledAiTools } from "../components/aiTools";
// 引入图片选择器组件
import { ImagePicker } from "../components/ImagePicker";
import { behaviorColors } from "../components/behaviorPresentation";
import { imageUploadAcceptValue } from "../components/imageUploadAccept";
// 引入设备未就绪时的统一拦截组件
import { useMonitorDeviceGate } from "../components/MonitorDeviceGate";
// 引入展示位选择器组件
import { SlotPicker } from "../components/SlotPicker";
// 引入维护当前工具 Tab、自动回退到可见工具的共享 hook
import { useActiveVisibleTool } from "../hooks/useActiveVisibleTool";
// 引入监控设备连接状态 hook
import { useMonitorConnection } from "../hooks/useMonitorConnection";
import { useDirectImageUpload } from "../hooks/useDirectImageUpload";
import { useSaveAiProfileDraft } from "../hooks/useSaveAiProfileDraft";
// 引入查询键与预定义的查询配置
import {
  aiProfilesQuery,
  monitorCapabilitiesQuery,
  monitorSettingsQuery,
  remoteImagesQuery,
} from "../queries/monitor";
import { useI18n } from "../../../shared/i18n";

export function AiManagementPage() {
  const { t } = useI18n();
  const monitorSettings = useQuery(monitorSettingsQuery);
  const capabilities = useQuery(monitorCapabilitiesQuery);
  // 当前选择、在线状态和可用性均来自 Rust 的单一设备快照。
  const {
    snapshot,
    hasConfiguredDevice,
    hasAvailableDevice,
    isPending: monitorPending,
  } = useMonitorConnection();
  const selectedDeviceId = snapshot.data?.selectedDeviceId ?? "";
  // Rust 为全部工具返回“已保存值或领域默认值”的完整草稿列表。
  const profiles = useQuery({
    ...aiProfilesQuery(selectedDeviceId),
    enabled: hasConfiguredDevice && hasAvailableDevice,
  });
  // 查询远端图片列表（对接 list_remote_images 命令），仅在设备已配置且可用时才发起请求
  const images = useQuery({
    ...remoteImagesQuery(selectedDeviceId),
    enabled: hasConfiguredDevice && hasAvailableDevice,
  });
  // 设置页勾选的 AI 客户端列表；未加载完成前视为空，避免闪现全部工具
  const enabledTools = monitorSettings.data?.enabledAiTools ?? [];
  // 按固定顺序过滤出当前可见的工具，仅在勾选集合变化时重新计算
  const visibleTools = useMemo(
    () => enabledAiTools(capabilities.data?.aiTools ?? [], enabledTools),
    [capabilities.data?.aiTools, enabledTools],
  );
  // 当前选中的 AI 工具 Tab；已选工具被取消勾选后自动回退到第一个可见工具
  const [activeTool, setActiveTool] = useActiveVisibleTool(visibleTools);
  // 各工具的展示配置草稿（未保存前的编辑态数据）
  const [drafts, setDrafts] = useState<
    Partial<Record<AiTool, AiProfileDraft>>
  >({});
  const [draftDeviceId, setDraftDeviceId] = useState<string | null>(null);
  const draft =
    activeTool && draftDeviceId === selectedDeviceId
      ? drafts[activeTool]
      : undefined;
  // 当前工具已选择图片的行为数量
  const configuredBehaviorCount = (draft?.hooks ?? []).filter(
    (hook) => hook.image.length > 0,
  ).length;
  // 仅用于即时展示完成度；保存合法性仍由 Rust 校验。
  const isComplete =
    draft !== undefined && configuredBehaviorCount === draft.hooks.length;

  // 后端列表已经覆盖全部工具，前端只转换为便于控件编辑的查找表。
  useEffect(() => {
    if (
      !profiles.data ||
      profiles.data.expectedDeviceId !== selectedDeviceId ||
      profiles.data.expectedDeviceId === draftDeviceId
    ) {
      return;
    }
    setDrafts(
      Object.fromEntries(
        profiles.data.drafts.map((profile) => [profile.tool, profile]),
      ) as Partial<Record<AiTool, AiProfileDraft>>,
    );
    setDraftDeviceId(profiles.data.expectedDeviceId);
  }, [draftDeviceId, profiles.data, selectedDeviceId]);

  const save = useSaveAiProfileDraft(setDrafts);

  const { upload, uploadedSelection, clearUploadedSelection } =
    useDirectImageUpload({
      setDrafts,
      onDraftChange: save.reset,
    });

  // 更新当前激活工具的草稿：先清空 save 的结果状态，再写入新的草稿内容
  const updateDraft = (next: AiProfileDraft) => {
    if (!activeTool) return;
    save.reset();
    setDrafts((current) => ({ ...current, [activeTool]: next }));
  };

  // 更新某个行为对应 hook 的单个字段（图片或文本内容）
  const updateHookField = (
    behaviorValue: HookBehavior,
    field: "image" | "content",
    value: string,
  ) => {
    if (!draft) return;
    updateDraft({
      ...draft,
      hooks: draft.hooks.map((item) =>
        item.behavior === behaviorValue ? { ...item, [field]: value } : item,
      ),
    });
  };

  const blockingError =
    profiles.error ??
    capabilities.error ??
    snapshot.error ??
    monitorSettings.error ??
    images.error;
  const mutationError = upload.error ?? save.error;
  // 可供选择的远端图片列表，查询未返回数据时兜底为空数组
  const availableImages = images.data?.images ?? [];
  const uploadAccept = imageUploadAcceptValue(
    capabilities.data?.imageUploadAccept,
  );
  // 查询失败优先于门禁/空态展示，避免把真实后端错误伪装成“未配置”。
  if (blockingError) return <Alert color="red">{blockingError.message}</Alert>;

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

  if (!activeTool || !draft || !draftDeviceId || !capabilities.data) {
    return (
      <Stack align="center" py="xl">
        <Loader size="sm" />
      </Stack>
    );
  }

  return (
    <Stack gap="md">
      {mutationError && <Alert color="red">{mutationError.message}</Alert>}
      {uploadedSelection?.tool === activeTool &&
        uploadedSelection.deviceId ===
          snapshot.data?.selectedDeviceId && (
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
                  min={capabilities.data.profileSlot.min}
                  max={capabilities.data.profileSlot.max}
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
                  {t("monitor.configuredCount", { configured: configuredBehaviorCount, total: draft.hooks.length })}
                </Badge>
              </Group>

              {!images.isPending && availableImages.length === 0 && (
                <Alert color="yellow" variant="light">
                  {t("monitor.noImages")}
                </Alert>
              )}

              <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="lg">
                {draft.hooks.map((hook) => {
                  const behaviorValue = hook.behavior;
                  return (
                    <Card
                      key={behaviorValue}
                      withBorder
                      className="behavior-card"
                      data-behavior={behaviorValue}
                      p={0}
                    >
                      <div className="behavior-card-header">
                        <Group justify="space-between" wrap="nowrap">
                          <Group gap="sm" wrap="nowrap">
                            <span className="behavior-card-status" />
                            <div>
                              <Text fw={700}>{t(`behavior.${behaviorValue}`)}</Text>
                              <Text size="xs" c="dimmed" mt={1}>
                                {t("monitor.stateDescription")}
                              </Text>
                            </div>
                          </Group>
                          <Badge
                            color={
                              hook.image ? "teal" : behaviorColors[behaviorValue]
                            }
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
                          counts={images.data?.counts}
                          uploadAccept={uploadAccept}
                          value={hook.image}
                          disabled={images.isPending || upload.isPending}
                          uploading={
                            upload.isPending &&
                            upload.variables?.tool === tool.value &&
                            upload.variables?.behavior === behaviorValue
                          }
                          onChange={(value) => {
                            clearUploadedSelection();
                            updateHookField(behaviorValue, "image", value);
                          }}
                          onUpload={(file) => {
                            const deviceId = draftDeviceId;
                            if (!deviceId) return;
                            upload.mutate({
                              file,
                              tool: tool.value,
                              behavior: behaviorValue,
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
                              behaviorValue,
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
                        : t("monitor.remaining", { count: draft.hooks.length - configuredBehaviorCount })}
                    </Text>
                  </div>
                  <Group>
                    {save.isSuccess &&
                      save.data.tool === activeTool &&
                      save.variables.expectedDeviceId === draftDeviceId && (
                      <Badge variant="light" color="teal">
                        {t("common.saved")}
                      </Badge>
                      )}
                    <Button
                      leftSection={<LineIcon name="check" size={17} />}
                      onClick={() =>
                        save.mutate({
                          profile: draft,
                          expectedDeviceId: draftDeviceId,
                        })
                      }
                      loading={save.isPending}
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
