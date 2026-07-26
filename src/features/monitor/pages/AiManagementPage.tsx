// 引入 Mantine UI 组件：Alert 提示、Badge 徽标、Button 按钮、Card 卡片、分组/堆叠布局、Tabs 选项卡、文本与输入控件
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
// 引入 TanStack Query 的 mutation/query hooks 及 queryClient，用于对接 Rust 后端命令
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
// 引入 React 的副作用、引用和状态 hooks
import { useEffect, useRef, useState } from "react";
// 引入本 feature 的类型定义
import type {
  AiProfile,
  AiTool,
  HookBehavior,
  HookConfigLocation,
} from "../api/monitor";
// 引入本 feature 对接 Rust 命令的类型化 API 函数
import {
  chooseHookConfigDirectory,
  saveAiProfile,
  saveHookConfigDirectory,
  writeHookConfig,
} from "../api/monitor";
// 引入查询键与预定义的查询配置
import {
  aiProfilesQuery,
  hookConfigLocationsQuery,
  monitorKeys,
  remoteImagesQuery,
} from "../queries/monitor";
// 引入通用图标组件
import { LineIcon } from "../../../shared/ui/LineIcon";
// 引入监控设备连接状态 hook
import { useMonitorConnection } from "../hooks/useMonitorConnection";
// 引入设备未就绪时的统一拦截组件
import { monitorDeviceGate } from "../components/MonitorDeviceGate";
// 引入图片选择器组件
import { ImagePicker } from "../components/ImagePicker";
// 引入展示位选择器组件
import { SlotPicker } from "../components/SlotPicker";

// 可配置的 AI 工具列表：取值与展示名的映射，用于渲染 Tabs
const tools: Array<{ value: AiTool; label: string }> = [
  { value: "codex", label: "Codex" },
  { value: "claudeCode", label: "Claude Code" },
  { value: "cursor", label: "Cursor" },
];

// 可配置的 hook 行为列表：取值、展示名与徽标颜色，用于渲染各行为的展示配置卡片
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

// 构造三个工具（codex/claudeCode/cursor）各自的初始空白草稿集合，用作 drafts 状态的初始值
function initialDrafts(): Record<AiTool, AiProfile> {
  return {
    codex: emptyProfile("codex"),
    claudeCode: emptyProfile("claudeCode"),
    cursor: emptyProfile("cursor"),
  };
}

// 构造三个工具各自的初始空目录草稿，用作 directoryDrafts 状态的初始值
function initialDirectoryDrafts(): Record<AiTool, string> {
  return {
    codex: "",
    claudeCode: "",
    cursor: "",
  };
}

export function AiManagementPage() {
  // 获取 QueryClient 实例，用于在 mutation 成功后手动写入缓存
  const queryClient = useQueryClient();
  // 查询当前设备下所有 AI 工具的 Profile 列表（对接 list_ai_profiles 命令）
  const profiles = useQuery(aiProfilesQuery);
  // 查询各工具的 hook 配置文件目录信息（对接 list_hook_config_locations 命令）
  const hookConfigLocations = useQuery(hookConfigLocationsQuery);
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
  // 当前选中的 AI 工具 Tab
  const [activeTool, setActiveTool] = useState<AiTool>("codex");
  // 各工具的展示配置草稿（未保存前的编辑态数据）
  const [drafts, setDrafts] =
    useState<Record<AiTool, AiProfile>>(initialDrafts);
  // 标记目录草稿是否已经用后端数据初始化过一次，避免用户编辑后被远端数据覆盖
  const locationsInitialized = useRef(false);
  // 各工具的 hook 配置目录草稿（未保存前的编辑态数据）
  const [directoryDrafts, setDirectoryDrafts] = useState<
    Record<AiTool, string>
  >(initialDirectoryDrafts);
  // 记录当前正在通过系统对话框选择目录的工具（用于按钮 loading 状态）
  const [selectingDirectory, setSelectingDirectory] =
    useState<AiTool | null>(null);
  // 目录选择框失败时的错误信息
  const [directoryPickerError, setDirectoryPickerError] = useState<
    string | null
  >(null);
  // 当前激活工具对应的草稿数据
  const draft = drafts[activeTool];
  // 判断当前草稿是否“完整”：每种行为都已配置齐全，且都选择了图片
  const isComplete =
    draft.hooks.length === behaviors.length &&
    draft.hooks.every((hook) => hook.image.length > 0);

  // 当远端 Profile 列表数据到达后，用远端数据重建 drafts（覆盖本地未保存的初始占位数据）
  useEffect(() => {
    if (!profiles.data) return;
    setDrafts(() => {
      const next = initialDrafts();
      for (const profile of profiles.data) next[profile.tool] = profile;
      return next;
    });
  }, [profiles.data]);

  // 当 hook 配置目录数据首次到达时，用远端目录初始化本地目录草稿；之后不再自动覆盖，避免打断用户编辑
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

  // 保存展示配置 Profile 的 mutation：对接 save_ai_profile 命令；成功后直接把返回结果合并进 profiles 查询缓存，替换同工具的旧记录
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

  // 保存 hook 配置目录的 mutation：对接 save_hook_config_directory 命令；成功后更新 hookConfigLocations 缓存、同步本地目录草稿，并重置写入结果状态
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

  // 写入 hook 配置文件的 mutation：对接 write_hook_config 命令，无成功回调（结果直接用 write.data 在 JSX 中展示）
  const write = useMutation({
    mutationFn: writeHookConfig,
  });

  // 更新当前激活工具的草稿：先清空 save/write 的结果状态，再写入新的草稿内容
  const updateDraft = (next: AiProfile) => {
    save.reset();
    write.reset();
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

  // 是否刚成功保存了当前激活工具的展示配置（用于显示“已保存”徽标）
  const saved = save.isSuccess && save.data.tool === activeTool;
  // 汇总所有相关查询/mutation 的错误，任意一个出错就展示错误提示
  const error =
    profiles.error ??
    devices.error ??
    monitorSettings.error ??
    images.error ??
    hookConfigLocations.error ??
    saveDirectory.error ??
    save.error ??
    write.error;
  // 可供选择的远端图片列表，查询未返回数据时兜底为空数组
  const availableImages = images.data ?? [];

  // 设备门禁：设备未配置或不可用时，返回统一的拦截提示，阻止渲染正式页面内容
  const deviceGate = monitorDeviceGate({
    isPending: monitorPending,
    hasConfiguredDevice,
    hasAvailableDevice,
    featureLabel: "监控管理",
  });
  if (deviceGate) return deviceGate;

  return (
    <Stack gap="md">
      {/* 汇总错误提示 */}
      {error && <Alert color="red">{error.message}</Alert>}
      {/* 目录选择框自身的错误提示 */}
      {directoryPickerError && (
        <Alert color="red">{directoryPickerError}</Alert>
      )}

      {/* 按 AI 工具分栏的选项卡；切换工具时清空各 mutation 的结果状态与目录选择错误 */}
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
        {/* 选项卡标签栏 */}
        <Tabs.List grow>
          {tools.map((tool) => (
            <Tabs.Tab key={tool.value} value={tool.value}>
              {tool.label}
            </Tabs.Tab>
          ))}
        </Tabs.List>

        {/* 为每个工具渲染一个选项卡面板，内容包含目录配置、展示位选择、行为图片/文案配置、保存与写入按钮 */}
        {tools.map((tool) => {
          // 当前工具对应的远端目录信息
          const location = hookConfigLocations.data?.find(
            (item) => item.tool === tool.value,
          );
          // 当前工具的目录草稿
          const directoryDraft = directoryDrafts[tool.value];
          // 目录草稿是否与远端已保存目录不一致（存在未保存修改）
          const pathDirty =
            Boolean(location) &&
            directoryDraft.trim() !== location?.directory;
          // 当前工具已保存的远端 Profile（用于判断草稿是否有未保存修改、是否可写入）
          const savedProfile = profiles.data?.find(
            (profile) => profile.tool === tool.value,
          );
          // 当前草稿与已保存 Profile 是否不一致（存在未保存修改）
          const profileDirty =
            Boolean(savedProfile) &&
            JSON.stringify(draft) !== JSON.stringify(savedProfile);
          // 是否刚成功写入了当前工具的 hook 配置
          const written =
            write.isSuccess && write.data.tool === tool.value;

          return (
            <Tabs.Panel key={tool.value} value={tool.value} pt="md">
              <Card withBorder className="surface-card" p="md">
                <Stack gap="md">
                {/* 配置目录说明文案 */}
                <div>
                  <Text fw={650}>配置目录</Text>
                  <Text size="sm" c="dimmed" mt={3}>
                    AIMonitor 会将本机中继规则写入该工具的配置文件。
                  </Text>
                </div>

                {/* 配置目录输入框及“选择目录/保存路径/恢复默认”操作按钮组 */}
                <Group align="center">
                  <TextInput
                    aria-label={`${tool.label} 配置目录`}
                    placeholder="选择或输入绝对目录"
                    value={directoryDraft}
                    disabled={hookConfigLocations.isPending}
                    onChange={(event) => {
                      // 手动编辑目录时，清空保存/写入结果状态与目录选择错误
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
                      // 弹出系统目录选择框，选择成功后写入目录草稿；失败则记录错误信息
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
                    // 保存当前工具的目录草稿到后端
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
                    // 通过保存空字符串目录来恢复为默认目录
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

                {/* 目录草稿未保存时的提示 */}
                {pathDirty && (
                  <Alert color="yellow" variant="light">
                    路径尚未保存。保存后写入会切换到新目录；旧文件不会被移动或删除。
                  </Alert>
                )}

                {/* 展示位选择器：选择该工具展示内容在屏幕上的位置 */}
                <SlotPicker
                  value={draft.slot}
                  onChange={(slot) => updateDraft({ ...draft, slot })}
                />

                {/* 行为展示说明文案 */}
                <div>
                  <Text fw={650}>行为展示</Text>
                  <Text size="sm" c="dimmed" mt={3}>
                    为每种行为选择图片；显示内容可按需填写。
                  </Text>
                </div>

                {/* 远端暂无可选图片时的提示 */}
                {!images.isPending && availableImages.length === 0 && (
                  <Alert color="yellow" variant="light">
                    暂无可选图片，请先到“图片管理”上传至少一张图片。
                  </Alert>
                )}

                {/* 每种行为一张卡片：包含行为徽标、图片选择器、文本内容输入框 */}
                <SimpleGrid cols={{ base: 1, md: 2 }} spacing="md">
                  {behaviors.map((behavior) => {
                    // 找到当前草稿中该行为对应的 hook，找不到则用空白占位
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
                          {/* 行为名称徽标，颜色随行为类型变化 */}
                          <Badge
                            color={behavior.color}
                            variant="light"
                            w="fit-content"
                          >
                            {behavior.label}
                          </Badge>

                          {/* 该行为对应展示图片的选择器 */}
                          <ImagePicker
                            images={availableImages}
                            value={hook.image}
                            disabled={images.isPending}
                            onChange={(value) =>
                              updateHookField(behavior.value, "image", value)
                            }
                          />

                          {/* 该行为对应的补充文本内容输入框 */}
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

                {/* 底部操作区：展示已保存徽标、保存展示配置按钮、写入结果徽标、写入 Hooks 配置按钮 */}
                <Group justify="flex-end">
                  {saved && (
                    <Badge variant="light" color="teal">
                      已保存
                    </Badge>
                  )}
                  <Button
                    leftSection={<LineIcon name="check" size={17} />}
                    // 保存当前激活工具的展示配置草稿
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
                    // 将该工具已保存的 Profile 写入其 hook 配置文件
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

                {/* 展示配置有未保存修改时的提示，需先保存才能写入 */}
                {profileDirty && (
                  <Alert color="yellow" variant="light">
                    展示配置有未保存修改。请先保存，再写入 Hooks 配置。
                  </Alert>
                )}

                {/* 该工具尚未保存过展示配置时的提示 */}
                {!profiles.isPending && !savedProfile && (
                  <Alert color="yellow" variant="light">
                    尚未保存 {tool.label} 的展示配置。完成图片选择并保存后，即可写入 Hooks 配置。
                  </Alert>
                )}

                {/* 仅针对 Codex：写入成功后提示用户后续需要在 Codex 中手动确认/信任配置 */}
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
