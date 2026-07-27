// 引入 Mantine UI 组件：Alert 提示、Badge 徽标、Button 按钮、Card 卡片、分组/堆叠布局、Tabs 选项卡与输入控件
import {
  Alert,
  Badge,
  Button,
  Card,
  Group,
  Stack,
  Tabs,
  Text,
  TextInput,
  Title,
} from "@mantine/core";
// 引入 TanStack Query 的 mutation/query hooks 及 queryClient，用于对接 Rust 后端命令
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
// 引入 React 的副作用、记忆化、引用和状态 hooks
import { useEffect, useMemo, useRef, useState } from "react";
// 引入通用图标组件
import { LineIcon } from "../../../shared/ui/LineIcon";
// 引入本 feature 的类型定义
import type { AiTool, HookConfigLocation } from "../api/monitor";
// 引入本 feature 对接 Rust 命令的类型化 API 函数
import {
  chooseHookConfigDirectory,
  saveHookConfigDirectory,
  writeHookConfig,
} from "../api/monitor";
// 引入维护当前工具 Tab、自动回退到可见工具的共享 hook
import { useActiveVisibleTool } from "../hooks/useActiveVisibleTool";
// 引入查询键与预定义的查询配置
import {
  hookConfigLocationsQuery,
  monitorKeys,
} from "../queries/monitor";
// 引入全部受支持 AI 工具的取值/展示名映射与可见性过滤函数
import { AI_TOOLS, enabledAiTools } from "./aiTools";

// 按 AI_TOOLS 固定顺序为全部工具生成空白目录草稿，避免逐个手写全部取值
function initialDirectoryDrafts(): Record<AiTool, string> {
  return Object.fromEntries(
    AI_TOOLS.map(({ value }) => [value, ""]),
  ) as Record<AiTool, string>;
}

// 写入配置后给出的后续操作提示：内容与 Rust 侧各 HookProtocol 的
// requires_review/restart_required 语义对应，仅在此处维护具体的操作步骤文案。
function hookActivationGuidance(
  tool: AiTool,
  filename: string,
  configChanged: boolean,
): string | null {
  if (!configChanged) {
    return "配置没有变化；如状态仍未生效，请确认 AIMonitor 规则已启用。";
  }
  switch (tool) {
    case "codex":
      return `配置已写入 ${filename}。请在 Codex CLI 中运行 /hooks，审核并信任包含 AIMonitor 标识的新增规则，然后重启 Codex App 或创建新任务。`;
    case "workBuddy":
      return `配置已写入 ${filename}。请在 WorkBuddy 的 Hooks 配置面板中审核并信任包含 AIMonitor 标识的新增规则，然后创建新任务。`;
    case "codeBuddy":
      return `配置已写入 ${filename}。请在 CodeBuddy 中运行 /hooks 审核 AIMonitor 规则，然后重启 CodeBuddy 或创建新会话。`;
    case "openClaw":
      return `AIMonitor 插件已写入 ${filename} 所在目录。请依次运行 openclaw plugins enable aimonitor、openclaw config set plugins.entries.aimonitor.hooks.allowConversationAccess true 和 openclaw gateway restart。`;
    case "hermes":
      return `AIMonitor 插件已写入 ${filename} 所在目录。请运行 hermes plugins enable aimonitor，然后重启 Hermes 或创建新会话。`;
    default:
      return null;
  }
}

interface HooksManagementCardProps {
  enabledTools: readonly AiTool[];
}

export function HooksManagementCard({
  enabledTools,
}: HooksManagementCardProps) {
  // 获取 QueryClient 实例，用于在 mutation 成功后手动写入缓存
  const queryClient = useQueryClient();
  // 查询各工具的 hook 配置文件目录信息（对接 list_hook_config_locations 命令）
  const locations = useQuery(hookConfigLocationsQuery);
  // 按固定顺序过滤出当前可见的工具，仅在勾选集合变化时重新计算
  const visibleTools = useMemo(
    () => enabledAiTools(enabledTools),
    [enabledTools],
  );
  // 当前选中的 AI 工具 Tab；已选工具被取消勾选后自动回退到第一个可见工具
  const [activeTool, setActiveTool] = useActiveVisibleTool(visibleTools);
  // 标记目录草稿是否已经用后端数据初始化过一次，避免用户编辑后被远端数据覆盖
  const initialized = useRef(false);
  // 各工具的 hook 配置目录草稿（未保存前的编辑态数据）
  const [directoryDrafts, setDirectoryDrafts] = useState(initialDirectoryDrafts);
  // 记录当前正在通过系统对话框选择目录的工具（用于按钮 loading 状态）
  const [selectingDirectory, setSelectingDirectory] =
    useState<AiTool | null>(null);
  // 目录选择框失败时的错误信息
  const [pickerError, setPickerError] = useState<string | null>(null);

  // 当 hook 配置目录数据首次到达时，用远端目录初始化本地目录草稿；之后不再自动覆盖，避免打断用户编辑
  useEffect(() => {
    if (!locations.data || initialized.current) return;
    setDirectoryDrafts((current) => {
      const next = { ...current };
      for (const location of locations.data) {
        next[location.tool] = location.directory;
      }
      return next;
    });
    initialized.current = true;
  }, [locations.data]);

  // 写入 hook 配置文件的 mutation：对接 write_hook_config 命令，无成功回调（结果直接用 write.data 在 JSX 中展示）
  const write = useMutation({ mutationFn: writeHookConfig });
  // 保存 hook 配置目录的 mutation：对接 save_hook_config_directory 命令；成功后更新 locations 缓存、同步本地目录草稿，并重置写入结果状态
  const saveDirectory = useMutation({
    mutationFn: ({ tool, directory }: { tool: AiTool; directory: string }) =>
      saveHookConfigDirectory(tool, directory),
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

  // 汇总所有相关查询/mutation 的错误，任意一个出错就展示错误提示
  const error = locations.error ?? saveDirectory.error ?? write.error;

  return (
    <Card
      withBorder
      radius="lg"
      p="sm"
      className="surface-card settings-card hooks-management-card"
    >
      <Stack gap="sm">
        <div>
          <Title order={4}>Hooks 管理</Title>
          <Text size="xs" c="dimmed" mt={2}>
            配置各客户端的目录，并将本机中继规则写入对应的 Hooks 配置。
          </Text>
        </div>
        {error && <Alert color="red">{error.message}</Alert>}
        {pickerError && <Alert color="red">{pickerError}</Alert>}

        {visibleTools.length === 0 ? (
          <Alert color="blue" variant="light">
            请先在上方“AI 客户端”卡片中选择并保存至少一个客户端。
          </Alert>
        ) : (
          <Tabs
            value={activeTool}
            onChange={(value) => {
              if (!value) return;
              saveDirectory.reset();
              write.reset();
              setPickerError(null);
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

            {visibleTools.map((tool) => {
              const location = locations.data?.find(
                (item) => item.tool === tool.value,
              );
              const directoryDraft = directoryDrafts[tool.value];
              const pathDirty =
                Boolean(location) &&
                directoryDraft.trim() !== location?.directory;
              const writeResult =
                write.isSuccess && write.data?.tool === tool.value
                  ? write.data
                  : undefined;
              const guidance = writeResult
                ? hookActivationGuidance(
                    tool.value,
                    writeResult.filename,
                    writeResult.configChanged,
                  )
                : null;

              return (
                <Tabs.Panel key={tool.value} value={tool.value} pt="xs">
                  <Stack gap="xs">
                    <TextInput
                      size="xs"
                      label="配置目录"
                      description="AIMonitor 会将本机中继规则写入该目录下的客户端配置文件"
                      aria-label={`${tool.label} 配置目录`}
                      placeholder="选择或输入绝对目录"
                      value={directoryDraft}
                      disabled={locations.isPending}
                      onChange={(event) => {
                        const directory = event.currentTarget.value;
                        saveDirectory.reset();
                        write.reset();
                        setPickerError(null);
                        setDirectoryDrafts((current) => ({
                          ...current,
                          [tool.value]: directory,
                        }));
                      }}
                    />

                    <Group justify="space-between" align="center" wrap="wrap">
                      <Group gap="sm" wrap="wrap">
                        <Button
                          size="xs"
                          variant="default"
                          leftSection={<LineIcon name="edit" size={17} />}
                          loading={selectingDirectory === tool.value}
                          onClick={async () => {
                            setSelectingDirectory(tool.value);
                            setPickerError(null);
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
                              setPickerError(
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
                          size="xs"
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
                          size="xs"
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

                      <Group gap="sm">
                        {writeResult && (
                          <Badge variant="light" color="teal">
                            {writeResult.requiresReview
                              ? "已写入，待确认"
                              : writeResult.configChanged
                                ? "已写入"
                                : "配置无变化"}
                          </Badge>
                        )}
                        <Button
                          size="xs"
                          leftSection={<LineIcon name="check" size={17} />}
                          onClick={() => write.mutate(tool.value)}
                          loading={
                            write.isPending && write.variables === tool.value
                          }
                          disabled={
                            locations.isPending || pathDirty
                          }
                        >
                          写入 Hooks 配置
                        </Button>
                      </Group>
                    </Group>

                    {pathDirty && (
                      <Alert color="yellow" variant="light">
                        路径尚未保存。保存后写入会切换到新目录；旧文件不会被移动或删除。
                      </Alert>
                    )}

                    {guidance && writeResult && (
                      <Alert
                        color={
                          writeResult.requiresReview ||
                          writeResult.restartRequired
                            ? "yellow"
                            : "blue"
                        }
                        title={
                          writeResult.requiresReview
                            ? `还需要在 ${tool.label} 中信任配置`
                            : writeResult.restartRequired
                              ? `还需要重新加载 ${tool.label} 配置`
                              : `${tool.label} 配置没有变化`
                        }
                      >
                        {guidance}
                      </Alert>
                    )}
                  </Stack>
                </Tabs.Panel>
              );
            })}
          </Tabs>
        )}
      </Stack>
    </Card>
  );
}
