import {
  Alert,
  Badge,
  Button,
  Card,
  Group,
  NumberInput,
  Stack,
  Switch,
  Tabs,
  Text,
  TextInput,
  Title,
} from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import type {
  AiTool,
  HookConfigLocation,
} from "../api/monitor";
import {
  chooseHookConfigDirectory,
  saveDiscoveryInterval,
  saveHookConfigDirectory,
  saveMonitorUsername,
  updateAutostart,
  writeHookConfig,
} from "../api/monitor";
import {
  aiProfilesQuery,
  hookConfigLocationsQuery,
  monitorKeys,
  monitorSettingsQuery,
  runtimeOverviewQuery,
} from "../queries/monitor";
import { LineIcon } from "../../../shared/ui/LineIcon";

const tools: Array<{ value: AiTool; label: string }> = [
  { value: "codex", label: "Codex" },
  { value: "claudeCode", label: "Claude Code" },
  { value: "cursor", label: "Cursor" },
];

function initialDirectoryDrafts(): Record<AiTool, string> {
  return {
    codex: "",
    claudeCode: "",
    cursor: "",
  };
}

export function SettingsPage() {
  const queryClient = useQueryClient();
  const runtime = useQuery(runtimeOverviewQuery);
  const settings = useQuery(monitorSettingsQuery);
  const profiles = useQuery(aiProfilesQuery);
  const hookConfigLocations = useQuery(hookConfigLocationsQuery);
  const locationsInitialized = useRef(false);
  const [activeTool, setActiveTool] = useState<AiTool>("codex");
  const [directoryDrafts, setDirectoryDrafts] = useState<
    Record<AiTool, string>
  >(initialDirectoryDrafts);
  const [selectingDirectory, setSelectingDirectory] =
    useState<AiTool | null>(null);
  const [directoryPickerError, setDirectoryPickerError] = useState<
    string | null
  >(null);
  const [username, setUsername] = useState("");
  const [discoveryIntervalMinutes, setDiscoveryIntervalMinutes] = useState(1);

  useEffect(() => {
    if (settings.data) setUsername(settings.data.username);
  }, [settings.data]);

  useEffect(() => {
    if (settings.data) {
      setDiscoveryIntervalMinutes(settings.data.discoveryIntervalMinutes);
    }
  }, [settings.data]);

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

  const autostart = useMutation({
    mutationFn: updateAutostart,
    onSuccess: (data) => queryClient.setQueryData(monitorKeys.runtime(), data),
  });

  const saveUsername = useMutation({
    mutationFn: saveMonitorUsername,
    onSuccess: (data) =>
      queryClient.setQueryData(monitorKeys.settings(), data),
  });

  const saveInterval = useMutation({
    mutationFn: saveDiscoveryInterval,
    onSuccess: (data) =>
      queryClient.setQueryData(monitorKeys.settings(), data),
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

  const error =
    runtime.error ??
    settings.error ??
    profiles.error ??
    hookConfigLocations.error ??
    saveDirectory.error ??
    write.error ??
    autostart.error ??
    saveUsername.error ??
    saveInterval.error;

  return (
    <Stack gap="md">
      {error && <Alert color="red">{error.message}</Alert>}
      {directoryPickerError && (
        <Alert color="red">{directoryPickerError}</Alert>
      )}

      <Card withBorder radius="lg" p="md" className="surface-card">
        <Stack gap="md">
          <div>
            <Title order={3}>通用设置</Title>
            <Text size="sm" c="dimmed" mt={4}>
              显示用户名由所有 AIMonitor 设备共享，不随当前设备切换。
            </Text>
          </div>
          <Group align="flex-end" wrap="nowrap">
            <TextInput
              label="显示用户名"
              description="状态转发时显示在所有设备上的名称"
              placeholder="输入显示用户名"
              value={username}
              onChange={(event) => {
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
              onClick={() => saveUsername.mutate(username)}
              loading={saveUsername.isPending}
              disabled={
                !username.trim() || username.trim() === settings.data?.username
              }
            >
              保存用户名
            </Button>
          </Group>
          {saveUsername.isSuccess && (
            <Alert color="teal">显示用户名已保存，并会用于所有设备。</Alert>
          )}

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
                saveInterval.reset();
                setDiscoveryIntervalMinutes(
                  typeof value === "number" ? value : 1,
                );
              }}
              disabled={settings.isPending}
              style={{ flex: "1 1 260px" }}
            />
            <Button
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
          {saveInterval.isSuccess && (
            <Alert color="teal">自动检查间隔已保存，立即生效。</Alert>
          )}

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

      <Card withBorder radius="lg" p="md" className="surface-card">
        <Stack gap="md">
          <div>
            <Title order={3}>AI Hooks 配置</Title>
            <Text size="sm" c="dimmed" mt={4}>
              选择各 AI 工具的配置目录，并将 AIMonitor 本机中继规则写入配置文件。
              展示位置、图片和文案请先在“AI 管理”中保存。
            </Text>
          </div>

          <Tabs
            value={activeTool}
            onChange={(value) => {
              if (!value) return;
              write.reset();
              saveDirectory.reset();
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
              const profileConfigured = profiles.data?.some(
                (profile) => profile.tool === tool.value,
              );
              const written =
                write.isSuccess && write.data.tool === tool.value;

              return (
                <Tabs.Panel key={tool.value} value={tool.value} pt="md">
                  <Stack gap="md">
                    <Stack gap="md">
                      <Group align="center">
                        <Text fw={650} miw={72}>
                          配置目录
                        </Text>
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
                          style={{ flex: "1 1 320px" }}
                        />
                      </Group>

                      {pathDirty && (
                        <Alert color="yellow" variant="light">
                          路径尚未保存。保存后，状态检查和写入都会切换到新目录；
                          旧文件不会被移动或删除。
                        </Alert>
                      )}

                      <Group justify="space-between">
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
                        <Group>
                          <Button
                            variant="default"
                            leftSection={<LineIcon name="edit" size={17} />}
                            loading={selectingDirectory === tool.value}
                            onClick={async () => {
                              setSelectingDirectory(tool.value);
                              setDirectoryPickerError(null);
                              try {
                                const selected =
                                  await chooseHookConfigDirectory(
                                    directoryDraft ||
                                      location?.directory ||
                                      "",
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
                        </Group>
                      </Group>
                    </Stack>

                    {!profiles.isPending && !profileConfigured && (
                      <Alert color="yellow" variant="light">
                        尚未保存 {tool.label} 的展示配置。请先前往“AI 管理”完成配置。
                      </Alert>
                    )}

                    <Group justify="flex-end">
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
                        leftSection={<LineIcon name="check" size={17} />}
                        onClick={() => write.mutate(tool.value)}
                        loading={
                          write.isPending &&
                          write.variables === tool.value
                        }
                        disabled={
                          hookConfigLocations.isPending ||
                          pathDirty ||
                          !profileConfigured
                        }
                      >
                        写入 Hooks 配置
                      </Button>
                    </Group>

                    {written && tool.value === "codex" && (
                      <Alert
                        color={
                          write.data.requiresReview ? "yellow" : "blue"
                        }
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
                </Tabs.Panel>
              );
            })}
          </Tabs>
        </Stack>
      </Card>
    </Stack>
  );
}
