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
  discoverySource:
    | "mdns"
    | "udpBroadcast"
    | "savedAddress"
    | "manualAddress";
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
  requiresReview: boolean;
  restartRequired: boolean;
}

export interface LocalHookConfig {
  tool: AiTool;
  filename: string;
  exists: boolean;
  valid: boolean;
  stableRunner: boolean;
  error: string;
  managedTargets: string[];
  content: string;
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

export function saveManualMonitorSettings(
  name: string,
  baseUrl: string,
  username: string,
): Promise<MonitorSettings> {
  return invokeCommand<MonitorSettings>("save_manual_monitor_settings", {
    name,
    baseUrl,
    username,
  });
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
