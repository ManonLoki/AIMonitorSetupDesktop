import { open } from "@tauri-apps/plugin-dialog";
import { invokeCommand } from "../../../shared/tauri/invoke-command";

export interface MonitorSettings {
  baseUrl: string;
  username: string;
  deviceId: string;
  deviceName: string;
}

export interface DiscoveredMonitorDevice {
  id: string;
  name: string;
  apiVersion: string;
  baseUrl: string;
  path: string;
  /** 设备是如何被找到的：mDNS 优先，失败回退 UDP 广播，再回退已保存地址。 */
  discoverySource: "mdns" | "udpBroadcast" | "savedAddress";
}

export interface ConnectionStatus {
  reachable: boolean;
  baseUrl: string;
  message: string;
}

export interface RemoteImage {
  filename: string;
  mimeType: string;
  image: string;
}

export type AiTool = "codex" | "claudeCode" | "cursor";

export type HookBehavior = "idle" | "running" | "asking" | "error";

export interface HookContent {
  behavior: HookBehavior;
  content: string;
  image: string;
}

export interface AiProfile {
  tool: AiTool;
  /** 在展示屏上的显示位置，取值范围 1-25。 */
  slot: number;
  hooks: HookContent[];
}

export interface HookConfigPreview {
  filename: string;
  content: string;
}

export interface HookConfigWriteResult {
  profile: AiProfile;
  filename: string;
  configChanged: boolean;
  /** 仅 Codex 且配置发生变化时为真：Codex 不会热加载 hooks.json，需提示用户手动确认。 */
  requiresReview: boolean;
  /** 仅 Codex 且配置发生变化时为真：需提示用户重启 Codex 才能生效。 */
  restartRequired: boolean;
}

export interface LocalHookConfig {
  tool: AiTool;
  filename: string;
  exists: boolean;
  valid: boolean;
  /** 现有配置里的托管条目是否已指向当前版本的 runner 脚本路径（无需迁移）。 */
  stableRunner: boolean;
  error: string;
  /** 现有配置中由本工具托管写入的事件名列表。 */
  managedTargets: string[];
  content: string;
}

export interface HookConfigLocation {
  tool: AiTool;
  directory: string;
  configPath: string;
  isCustom: boolean;
}

export function getMonitorSettings(): Promise<MonitorSettings> {
  return invokeCommand<MonitorSettings>("get_monitor_settings");
}

export function saveMonitorSettings(
  device: DiscoveredMonitorDevice,
  username: string,
): Promise<MonitorSettings> {
  return invokeCommand<MonitorSettings>("save_monitor_settings", {
    device,
    username,
  });
}

export function discoverMonitorDevices(): Promise<DiscoveredMonitorDevice[]> {
  return invokeCommand<DiscoveredMonitorDevice[]>("discover_monitor_devices");
}

export function checkMonitorConnection(
  baseUrl?: string,
): Promise<ConnectionStatus> {
  return invokeCommand<ConnectionStatus>("check_monitor_connection", {
    baseUrl,
  });
}

export function listRemoteImages(): Promise<RemoteImage[]> {
  return invokeCommand<RemoteImage[]>("list_remote_images");
}

export async function uploadRemoteImages(files: File[]): Promise<string[]> {
  const images = await Promise.all(
    files.map(async (file) => ({
      filename: file.name,
      mimeType: file.type,
      bytes: Array.from(new Uint8Array(await file.arrayBuffer())),
    })),
  );

  return invokeCommand<string[]>("upload_remote_images", { images });
}

export function deleteRemoteImage(filename: string): Promise<void> {
  return invokeCommand<void>("delete_remote_image", { filename });
}

export function listAiProfiles(): Promise<AiProfile[]> {
  return invokeCommand<AiProfile[]>("list_ai_profiles");
}

export function listHookConfigLocations(): Promise<HookConfigLocation[]> {
  return invokeCommand<HookConfigLocation[]>("list_hook_config_locations");
}

export function saveHookConfigDirectory(
  tool: AiTool,
  directory: string,
): Promise<HookConfigLocation> {
  return invokeCommand<HookConfigLocation>("save_hook_config_directory", {
    tool,
    directory,
  });
}

export async function chooseHookConfigDirectory(
  defaultPath: string,
): Promise<string | null> {
  return open({
    title: "选择 Hooks 配置目录",
    directory: true,
    multiple: false,
    defaultPath,
  });
}

export function writeAiProfile(
  profile: AiProfile,
): Promise<HookConfigWriteResult> {
  return invokeCommand<HookConfigWriteResult>("write_ai_profile", { profile });
}

export function previewHookConfig(
  profile: AiProfile,
): Promise<HookConfigPreview> {
  return invokeCommand<HookConfigPreview>("preview_hook_config", { profile });
}

export function listLocalHookConfigs(): Promise<LocalHookConfig[]> {
  return invokeCommand<LocalHookConfig[]>("list_local_hook_configs");
}
