import {
  Accordion,
  Alert,
  Badge,
  Button,
  Card,
  Code,
  Group,
  Loader,
  ScrollArea,
  SimpleGrid,
  Stack,
  Tabs,
  Text,
  Textarea,
} from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import type {
  AiProfile,
  AiTool,
  HookBehavior,
} from "../api/monitor";
import { writeAiProfile } from "../api/monitor";
import {
  aiProfilesQuery,
  hookPreviewQuery,
  localHookConfigsQuery,
  monitorKeys,
  remoteImagesQuery,
} from "../queries/monitor";
import { LineIcon } from "../../../shared/ui/LineIcon";
import { ImagePicker } from "../components/ImagePicker";
import { SlotPicker } from "../components/SlotPicker";

const tools: Array<{ value: AiTool; label: string }> = [
  { value: "codex", label: "Codex" },
  { value: "claudeCode", label: "Claude Code" },
  { value: "cursor", label: "Cursor" },
];

const behaviors: Array<{
  value: HookBehavior;
  label: string;
  description: string;
  color: string;
}> = [
  {
    value: "idle",
    label: "空闲",
    description: "客户端已启动，当前等待新的任务",
    color: "gray",
  },
  {
    value: "running",
    label: "运行中",
    description: "正在处理提示词或调用工具",
    color: "violet",
  },
  {
    value: "asking",
    label: "询问",
    description: "等待用户确认、授权或输入",
    color: "yellow",
  },
  {
    value: "error",
    label: "异常",
    description: "工具调用或任务执行失败",
    color: "red",
  },
];

function emptyProfile(tool: AiTool): AiProfile {
  return {
    tool,
    slot: 1,
    hooks: behaviors.map(({ value }) => ({
      behavior: value,
      content: "",
      image: "",
    })),
  };
}

function initialDrafts(): Record<AiTool, AiProfile> {
  return {
    codex: emptyProfile("codex"),
    claudeCode: emptyProfile("claudeCode"),
    cursor: emptyProfile("cursor"),
  };
}

export function AiManagementPage() {
  const queryClient = useQueryClient();
  const profiles = useQuery(aiProfilesQuery);
  const images = useQuery(remoteImagesQuery);
  const localConfigs = useQuery(localHookConfigsQuery);
  const initialized = useRef(false);
  const [activeTool, setActiveTool] = useState<AiTool>("codex");
  const [drafts, setDrafts] =
    useState<Record<AiTool, AiProfile>>(initialDrafts);
  const draft = drafts[activeTool];
  const isComplete =
    draft.hooks.length === behaviors.length &&
    draft.hooks.every((hook) => hook.image.length > 0);
  const preview = useQuery({
    ...hookPreviewQuery(draft),
    enabled: isComplete,
  });

  useEffect(() => {
    if (!profiles.data || initialized.current) return;
    setDrafts((current) => {
      const next = { ...current };
      for (const profile of profiles.data) next[profile.tool] = profile;
      return next;
    });
    initialized.current = true;
  }, [profiles.data]);

  const write = useMutation({
    mutationFn: writeAiProfile,
    onSuccess: ({ profile: writtenProfile }) => {
      queryClient.setQueryData<AiProfile[]>(
        monitorKeys.profiles(),
        (current = []) => [
          ...current.filter(
            (profile) => profile.tool !== writtenProfile.tool,
          ),
          writtenProfile,
        ],
      );
      void queryClient.invalidateQueries({
        queryKey: monitorKeys.localHookConfigs(),
      });
    },
  });

  const updateDraft = (next: AiProfile) => {
    write.reset();
    setDrafts((current) => ({ ...current, [activeTool]: next }));
  };

  const written =
    write.isSuccess && write.data.profile.tool === activeTool;
  const error =
    profiles.error ??
    images.error ??
    localConfigs.error ??
    write.error ??
    preview.error;
  const availableImages = images.data ?? [];

  return (
    <Stack gap="lg">
      {error && <Alert color="red">{error.message}</Alert>}

      <Tabs
        value={activeTool}
        onChange={(value) => {
          if (!value) return;
          write.reset();
          setActiveTool(value as AiTool);
        }}
        className="ai-tool-tabs"
      >
        <Tabs.List grow>
          {tools.map((tool) => (
            <Tabs.Tab key={tool.value} value={tool.value}>
              {tool.label}
            </Tabs.Tab>
          ))}
        </Tabs.List>

        {tools.map((tool) => (
          <Tabs.Panel key={tool.value} value={tool.value} pt="lg">
            <Stack gap="lg">
              <Card withBorder className="surface-card" p="lg">
                <Stack gap="lg">
                  <SlotPicker
                    value={draft.slot}
                    onChange={(slot) => updateDraft({ ...draft, slot })}
                  />

                  <div>
                    <Text fw={650}>行为展示</Text>
                    <Text size="sm" c="dimmed" mt={3}>
                      为每种行为选择图片；显示内容可按需填写。
                    </Text>
                  </div>

                  {!images.isPending && availableImages.length === 0 && (
                    <Alert color="yellow" variant="light">
                      暂无可选图片，请先到“图片管理”上传至少一张图片。
                    </Alert>
                  )}

                  <SimpleGrid cols={{ base: 1, md: 2 }} spacing="md">
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
                          p="md"
                        >
                          <Stack gap="md">
                            <Group justify="space-between" align="flex-start">
                              <div>
                                <Badge color={behavior.color} variant="light">
                                  {behavior.label}
                                </Badge>
                                <Text size="sm" c="dimmed" mt={7}>
                                  {activeTool === "codex" &&
                                  behavior.value === "error"
                                    ? "当前 Codex Desktop Hook 协议没有独立的 Error 事件，暂不自动触发"
                                    : behavior.description}
                                </Text>
                              </div>
                            </Group>

                            <ImagePicker
                              images={availableImages}
                              value={hook.image}
                              disabled={images.isPending}
                              onChange={(value) =>
                                updateDraft({
                                  ...draft,
                                  hooks: draft.hooks.map((item) =>
                                    item.behavior === behavior.value
                                      ? { ...item, image: value }
                                      : item,
                                  ),
                                })
                              }
                            />

                            <Textarea
                              label="内容"
                              description="可选"
                              placeholder="输入设备上显示的补充内容"
                              autosize
                              minRows={2}
                              value={hook.content}
                              onChange={(event) =>
                                updateDraft({
                                  ...draft,
                                  hooks: draft.hooks.map((item) =>
                                    item.behavior === behavior.value
                                      ? {
                                          ...item,
                                          content: event.currentTarget.value,
                                        }
                                      : item,
                                  ),
                                })
                              }
                            />
                          </Stack>
                        </Card>
                      );
                    })}
                  </SimpleGrid>

                  <Group justify="flex-end">
                    {written && (
                      <Badge variant="light" color="teal">
                        {write.data.requiresReview
                          ? "已写入，待 Codex 确认"
                          : "已写入"}
                      </Badge>
                    )}
                    <Button
                      leftSection={<LineIcon name="check" size={17} />}
                      onClick={() => write.mutate(draft)}
                      loading={write.isPending}
                      disabled={!isComplete}
                    >
                      写入 Hooks 配置
                    </Button>
                  </Group>

                  {written && activeTool === "codex" && (
                    <Alert
                      color={write.data.requiresReview ? "yellow" : "blue"}
                      title={
                        write.data.requiresReview
                          ? "还需要在 Codex 中信任配置"
                          : "Codex 配置没有变化"
                      }
                    >
                      {write.data.requiresReview
                        ? `配置已写入 ${write.data.filename}。请在 Codex CLI 中运行 /hooks，审核并信任包含 aimonitor-managed-hook 的新增规则，然后重启 Codex App 或创建新任务。此后修改展示配置不需要再次信任。`
                        : "如状态仍未生效，请在 Codex CLI 中运行 /hooks 确认规则已受信任，然后在 Codex App 中创建新任务。"}
                    </Alert>
                  )}
                </Stack>
              </Card>

              <Card withBorder className="surface-card" padding={0}>
                <Group justify="space-between" p="md">
                  <div>
                    <Text fw={650}>合并后将写入的 Hooks 配置</Text>
                    <Text size="xs" c="dimmed" mt={2}>
                      其他 APP 的配置会保持不变；旧版 AIMonitor
                      目标会清理并迁移为固定 Runner。后续修改设备、图片或文案
                      不会再次改变 Hook 信任哈希
                    </Text>
                  </div>
                  {preview.data && (
                    <Code>{preview.data.filename}</Code>
                  )}
                </Group>
                <div className="hook-preview">
                  {!isComplete ? (
                    <Text c="dimmed" size="sm">
                      为四种行为选择图片后显示预览
                    </Text>
                  ) : preview.isPending ? (
                    <Loader size="sm" />
                  ) : preview.data ? (
                    <ScrollArea type="auto">
                      <pre>{preview.data.content}</pre>
                    </ScrollArea>
                  ) : null}
                </div>
              </Card>
            </Stack>
          </Tabs.Panel>
        ))}
      </Tabs>

      <Card withBorder className="surface-card" p="lg">
        <Stack gap="md">
          <div>
            <Text fw={650}>本机历史配置</Text>
            <Text size="sm" c="dimmed" mt={3}>
              查看本机三个 AI 工具的实际配置文件，以及 AIMonitor
              曾写入的目标设备地址。
            </Text>
          </div>

          {localConfigs.isPending ? (
            <Loader size="sm" />
          ) : (
            <Accordion multiple variant="separated">
              {localConfigs.data?.map((config) => {
                const toolLabel =
                  tools.find((tool) => tool.value === config.tool)?.label ??
                  config.tool;
                return (
                  <Accordion.Item key={config.tool} value={config.tool}>
                    <Accordion.Control>
                      <Group justify="space-between" pr="md">
                        <div>
                          <Text fw={600}>{toolLabel}</Text>
                          <Code>{config.filename}</Code>
                        </div>
                        <Group gap="xs">
                          {!config.exists ? (
                            <Badge variant="light" color="gray">
                              尚未创建
                            </Badge>
                          ) : config.valid ? (
                            <Badge variant="light" color="teal">
                              配置有效
                            </Badge>
                          ) : (
                            <Badge variant="light" color="red">
                              格式异常
                            </Badge>
                          )}
                          {config.managedTargets.length > 0 && (
                            <Badge variant="light">
                              {config.managedTargets.length} 个目标
                            </Badge>
                          )}
                          {config.stableRunner && (
                            <Badge variant="light" color="blue">
                              稳定 Runner
                            </Badge>
                          )}
                        </Group>
                      </Group>
                    </Accordion.Control>
                    <Accordion.Panel>
                      <Stack gap="sm">
                        {config.error && (
                          <Alert color="red">{config.error}</Alert>
                        )}
                        {config.managedTargets.length > 0 && (
                          <div>
                            <Text size="xs" c="dimmed" mb={5}>
                              AIMonitor 历史目标
                            </Text>
                            <Group gap="xs">
                              {config.managedTargets.map((target) => (
                                <Code key={target}>{target}</Code>
                              ))}
                            </Group>
                          </div>
                        )}
                        {config.stableRunner && (
                          <Alert color="blue" variant="light">
                            已使用稳定 Runner。以后修改 AIMonitor
                            设备、槽位、图片或文案时，不需要重新信任这些 Hooks。
                          </Alert>
                        )}
                        {config.exists ? (
                          <div className="hook-preview">
                            <ScrollArea type="auto">
                              <pre>{config.content}</pre>
                            </ScrollArea>
                          </div>
                        ) : (
                          <Text size="sm" c="dimmed">
                            本机还没有这个配置文件。
                          </Text>
                        )}
                      </Stack>
                    </Accordion.Panel>
                  </Accordion.Item>
                );
              })}
            </Accordion>
          )}
        </Stack>
      </Card>
    </Stack>
  );
}
