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
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import type {
  AiProfile,
  AiTool,
  HookBehavior,
} from "../api/monitor";
import { saveAiProfile } from "../api/monitor";
import {
  aiProfilesQuery,
  monitorDevicesQuery,
  monitorKeys,
  monitorSettingsQuery,
  remoteImagesQuery,
} from "../queries/monitor";
import { LineIcon } from "../../../shared/ui/LineIcon";
import { DeviceConnectPanel } from "../components/DeviceConnectPanel";
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
  color: string;
}> = [
  { value: "idle", label: "空闲", color: "gray" },
  { value: "running", label: "运行中", color: "violet" },
  { value: "asking", label: "询问", color: "yellow" },
  { value: "error", label: "异常", color: "red" },
];

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
  const devices = useQuery(monitorDevicesQuery);
  const monitorSettings = useQuery(monitorSettingsQuery);
  const connectedDevice = devices.data?.find(
    (device) => device.id === monitorSettings.data?.deviceId,
  );
  const hasConnectedDevice = Boolean(connectedDevice);
  const images = useQuery({
    ...remoteImagesQuery,
    enabled: hasConnectedDevice,
  });
  const [activeTool, setActiveTool] = useState<AiTool>("codex");
  const [drafts, setDrafts] =
    useState<Record<AiTool, AiProfile>>(initialDrafts);
  const draft = drafts[activeTool];
  const isComplete =
    draft.hooks.length === behaviors.length &&
    draft.hooks.every((hook) => hook.image.length > 0);

  useEffect(() => {
    if (!profiles.data) return;
    setDrafts(() => {
      const next = initialDrafts();
      for (const profile of profiles.data) next[profile.tool] = profile;
      return next;
    });
  }, [profiles.data]);

  const save = useMutation({
    mutationFn: saveAiProfile,
    onSuccess: (savedProfile) => {
      queryClient.setQueryData<AiProfile[]>(
        monitorKeys.profiles(),
        (current = []) => [
          ...current.filter(
            (profile) => profile.tool !== savedProfile.tool,
          ),
          savedProfile,
        ],
      );
    },
  });

  const updateDraft = (next: AiProfile) => {
    save.reset();
    setDrafts((current) => ({ ...current, [activeTool]: next }));
  };

  const saved = save.isSuccess && save.data.tool === activeTool;
  const error =
    profiles.error ??
    devices.error ??
    monitorSettings.error ??
    images.error ??
    save.error;
  const availableImages = images.data ?? [];

  if (devices.isPending || monitorSettings.isPending) {
    return (
      <Stack align="center" py="xl">
        <Loader size="sm" />
        <Text size="sm" c="dimmed">
          正在发现 AIMonitor 设备…
        </Text>
      </Stack>
    );
  }

  if (!connectedDevice) {
    return (
      <Stack gap="lg" maw={860}>
        <Alert color="blue" title="先连接一台设备">
          首次使用需要先选择一台设备。显示用户名可在“设置”中统一配置，每台
          设备分别保存自己的 AI 展示配置。
        </Alert>
        <DeviceConnectPanel />
      </Stack>
    );
  }

  return (
    <Stack gap="md">
      {error && <Alert color="red">{error.message}</Alert>}

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
          {tools.map((tool) => (
            <Tabs.Tab key={tool.value} value={tool.value}>
              {tool.label}
            </Tabs.Tab>
          ))}
        </Tabs.List>

        {tools.map((tool) => (
          <Tabs.Panel key={tool.value} value={tool.value} pt="md">
            <Card withBorder className="surface-card" p="md">
              <Stack gap="md">
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
                        p="sm"
                      >
                        <Stack gap="sm">
                          <Badge
                            color={behavior.color}
                            variant="light"
                            w="fit-content"
                          >
                            {behavior.label}
                          </Badge>

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
                            label={
                              <span>
                                内容{" "}
                                <Text
                                  component="span"
                                  size="xs"
                                  fw={400}
                                  c="dimmed"
                                >
                                  可选
                                </Text>
                              </span>
                            }
                            placeholder="输入设备上显示的补充内容"
                            autosize
                            minRows={1}
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
                  {saved && (
                    <Badge variant="light" color="teal">
                      已保存
                    </Badge>
                  )}
                  <Button
                    leftSection={<LineIcon name="check" size={17} />}
                    onClick={() => save.mutate(draft)}
                    loading={save.isPending}
                    disabled={!isComplete}
                  >
                    保存展示配置
                  </Button>
                </Group>
              </Stack>
            </Card>
          </Tabs.Panel>
        ))}
      </Tabs>
    </Stack>
  );
}
