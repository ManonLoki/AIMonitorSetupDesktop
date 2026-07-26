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
  TextInput,
  Textarea,
} from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import type {
  AiProfile,
  AiTool,
  HookBehavior,
  HookConfigLocation,
} from "../api/monitor";
import {
  chooseHookConfigDirectory,
  saveAiProfile,
  saveHookConfigDirectory,
  writeHookConfig,
} from "../api/monitor";
import {
  aiProfilesQuery,
  hookConfigLocationsQuery,
  monitorKeys,
  remoteImagesQuery,
} from "../queries/monitor";
import { LineIcon } from "../../../shared/ui/LineIcon";
import { useMonitorConnection } from "../hooks/useMonitorConnection";
import { monitorDeviceGate } from "../components/MonitorDeviceGate";
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

function initialDirectoryDrafts(): Record<AiTool, string> {
  return {
    codex: "",
    claudeCode: "",
    cursor: "",
  };
}

export function AiManagementPage() {
  const queryClient = useQueryClient();
  const profiles = useQuery(aiProfilesQuery);
  const hookConfigLocations = useQuery(hookConfigLocationsQuery);
  const {
    settings: monitorSettings,
    devices,
    hasConfiguredDevice,
    hasAvailableDevice,
    isPending: monitorPending,
  } = useMonitorConnection();
  const images = useQuery({
    ...remoteImagesQuery,
    enabled: hasConfiguredDevice && hasAvailableDevice,
  });
  const [activeTool, setActiveTool] = useState<AiTool>("codex");
  const [drafts, setDrafts] =
    useState<Record<AiTool, AiProfile>>(initialDrafts);
  const locationsInitialized = useRef(false);
  const [directoryDrafts, setDirectoryDrafts] = useState<
    Record<AiTool, string>
  >(initialDirectoryDrafts);
  const [selectingDirectory, setSelectingDirectory] =
    useState<AiTool | null>(null);
  const [directoryPickerError, setDirectoryPickerError] = useState<
    string | null
  >(null);
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

  useEffect(() => {
    if (!hookConfigLocations.data || locationsInitialized.current) return;
    setDirectoryDrafts((current) => {
      const next = { ...current };
      for (const location of hookConfigLocations.data) {
        next[location.tool] = location.directory;
      }
      return next;
    });
    locationsInitialized.current = true;
  }, [hookConfigLocations.data]);

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

  const saveDirectory = useMutation({
    mutationFn: ({
      tool,
      directory,
    }: {
      tool: AiTool;
      directory: string;
    }) => saveHookConfigDirectory(tool, directory),
    onSuccess: (savedLocation) => {
      queryClient.setQueryData<HookConfigLocation[]>(
        monitorKeys.hookConfigLocations(),
        (current = []) => [
          ...current.filter(
            (location) => location.tool !== savedLocation.tool,
          ),
          savedLocation,
        ],
      );
      setDirectoryDrafts((current) => ({
        ...current,
        [savedLocation.tool]: savedLocation.directory,
      }));
      write.reset();
    },
  });

  const write = useMutation({
    mutationFn: writeHookConfig,
  });

  const updateDraft = (next: AiProfile) => {
    save.reset();
    write.reset();
    setDrafts((current) => ({ ...current, [activeTool]: next }));
  };

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

  const saved = save.isSuccess && save.data.tool === activeTool;
  const error =
    profiles.error ??
    devices.error ??
    monitorSettings.error ??
    images.error ??
    hookConfigLocations.error ??
    saveDirectory.error ??
    save.error ??
    write.error;
  const availableImages = images.data ?? [];

  const deviceGate = monitorDeviceGate({
    isPending: monitorPending,
    hasConfiguredDevice,
    hasAvailableDevice,
    featureLabel: "监控管理",
  });
  if (deviceGate) return deviceGate;

  return (
    <Stack gap="md">
      {error && <Alert color="red">{error.message}</Alert>}
      {directoryPickerError && (
        <Alert color="red">{directoryPickerError}</Alert>
      )}

      <Tabs
        value={activeTool}
        onChange={(value) => {
          if (!value) return;
          save.reset();
          saveDirectory.reset();
          write.reset();
          setDirectoryPickerError(null);
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

        {tools.map((tool) => {
          const location = hookConfigLocations.data?.find(
            (item) => item.tool === tool.value,
          );
          const directoryDraft = directoryDrafts[tool.value];
          const pathDirty =
            Boolean(location) &&
            directoryDraft.trim() !== location?.directory;
          const savedProfile = profiles.data?.find(
            (profile) => profile.tool === tool.value,
          );
          const profileDirty =
            Boolean(savedProfile) &&
            JSON.stringify(draft) !== JSON.stringify(savedProfile);
          const written =
            write.isSuccess && write.data.tool === tool.value;

          return (
            <Tabs.Panel key={tool.value} value={tool.value} pt="md">
              <Card withBorder className="surface-card" p="md">
                <Stack gap="md">
                <div>
                  <Text fw={650}>配置目录</Text>
                  <Text size="sm" c="dimmed" mt={3}>
                    AIMonitor 会将本机中继规则写入该工具的配置文件。
                  </Text>
                </div>

                <Group align="center">
                  <TextInput
                    aria-label={`${tool.label} 配置目录`}
                    placeholder="选择或输入绝对目录"
                    value={directoryDraft}
                    disabled={hookConfigLocations.isPending}
                    onChange={(event) => {
                      saveDirectory.reset();
                      write.reset();
                      setDirectoryPickerError(null);
                      setDirectoryDrafts((current) => ({
                        ...current,
                        [tool.value]: event.currentTarget.value,
                      }));
                    }}
                    style={{ flex: "1 1 360px" }}
                  />
                  <Button
                    variant="default"
                    leftSection={<LineIcon name="edit" size={17} />}
                    loading={selectingDirectory === tool.value}
                    onClick={async () => {
                      setSelectingDirectory(tool.value);
                      setDirectoryPickerError(null);
                      try {
                        const selected = await chooseHookConfigDirectory(
                          directoryDraft || location?.directory || "",
                        );
                        if (selected) {
                          write.reset();
                          setDirectoryDrafts((current) => ({
                            ...current,
                            [tool.value]: selected,
                          }));
                        }
                      } catch (error) {
                        setDirectoryPickerError(
                          error instanceof Error
                            ? error.message
                            : String(error),
                        );
                      } finally {
                        setSelectingDirectory(null);
                      }
                    }}
                  >
                    选择目录
                  </Button>
                  <Button
                    variant="default"
                    onClick={() =>
                      saveDirectory.mutate({
                        tool: tool.value,
                        directory: directoryDraft,
                      })
                    }
                    loading={
                      saveDirectory.isPending &&
                      saveDirectory.variables?.tool === tool.value &&
                      saveDirectory.variables.directory !== ""
                    }
                    disabled={!directoryDraft.trim() || !pathDirty}
                  >
                    保存路径
                  </Button>
                  <Button
                    variant="subtle"
                    color="gray"
                    onClick={() =>
                      saveDirectory.mutate({
                        tool: tool.value,
                        directory: "",
                      })
                    }
                    loading={
                      saveDirectory.isPending &&
                      saveDirectory.variables?.tool === tool.value &&
                      saveDirectory.variables.directory === ""
                    }
                    disabled={!location?.isCustom}
                  >
                    恢复默认
                  </Button>
                </Group>

                {pathDirty && (
                  <Alert color="yellow" variant="light">
                    路径尚未保存。保存后写入会切换到新目录；旧文件不会被移动或删除。
                  </Alert>
                )}

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
                              updateHookField(behavior.value, "image", value)
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
                  {written && (
                    <Badge variant="light" color="teal">
                      {write.data.requiresReview
                        ? "已写入，待 Codex 确认"
                        : write.data.configChanged
                          ? "已写入"
                          : "配置无变化"}
                    </Badge>
                  )}
                  <Button
                    variant="filled"
                    leftSection={<LineIcon name="check" size={17} />}
                    onClick={() => write.mutate(tool.value)}
                    loading={
                      write.isPending && write.variables === tool.value
                    }
                    disabled={
                      hookConfigLocations.isPending ||
                      pathDirty ||
                      !savedProfile ||
                      profileDirty
                    }
                  >
                    写入 Hooks 配置
                  </Button>
                </Group>

                {profileDirty && (
                  <Alert color="yellow" variant="light">
                    展示配置有未保存修改。请先保存，再写入 Hooks 配置。
                  </Alert>
                )}

                {!profiles.isPending && !savedProfile && (
                  <Alert color="yellow" variant="light">
                    尚未保存 {tool.label} 的展示配置。完成图片选择并保存后，即可写入 Hooks 配置。
                  </Alert>
                )}

                {written && tool.value === "codex" && (
                  <Alert
                    color={write.data.requiresReview ? "yellow" : "blue"}
                    title={
                      write.data.requiresReview
                        ? "还需要在 Codex 中信任配置"
                        : "Codex 配置没有变化"
                    }
                  >
                    {write.data.requiresReview
                      ? `配置已写入 ${write.data.filename}。请在 Codex CLI 中运行 /hooks，审核并信任包含 AIMonitor 标识的新增规则，然后重启 Codex App 或创建新任务。`
                      : "如状态仍未生效，请在 Codex CLI 中运行 /hooks 确认规则已受信任，然后在 Codex App 中创建新任务。"}
                  </Alert>
                )}
                </Stack>
              </Card>
            </Tabs.Panel>
          );
        })}
      </Tabs>
    </Stack>
  );
}
