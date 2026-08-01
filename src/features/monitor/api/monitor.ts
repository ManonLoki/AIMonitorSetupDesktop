// 从 Tauri 对话框插件引入 open，用于弹出系统文件夹选择框
import { open } from "@tauri-apps/plugin-dialog";
// 引入通用的 Tauri invoke 封装，所有与 Rust 后端的通信都通过它完成
import { invokeCommand } from "../../../shared/tauri/invoke-command";

// 监控设备的连接与基础设置，对应 Rust 侧 MonitorSettings DTO
export interface MonitorSettings {
  // 设备的基础访问地址（如 http://ip:port）
  baseUrl: string;
  // 当前登录/使用的用户名
  username: string;
  // 当前已选中设备的唯一标识
  deviceId: string;
  // 当前已选中设备的展示名称
  deviceName: string;
  /** 在线设备自动检查间隔（分钟），默认 1 分钟；保存后后台发现循环立即生效。 */
  discoveryIntervalMinutes: number;
  /** 在监控管理和 Hooks 管理中显示的 AI 客户端。 */
  enabledAiTools: AiTool[];
}

// Rust 领域层提供的 AI 工具目录项；成员、名称和顺序不再由前端维护。
export interface AiToolDescriptor {
  tool: AiTool;
  name: string;
}

// 前端控件可直接消费的闭区间业务约束。
export interface MonitorCapabilityRange {
  default: number;
  min: number;
  max: number;
}

// Rust 统一返回的监控静态能力，对应真实校验和 HookProtocol 元数据。
export interface MonitorCapabilities {
  aiTools: AiToolDescriptor[];
  discoveryInterval: MonitorCapabilityRange;
  profileSlot: MonitorCapabilityRange;
  hookBehaviors: HookBehavior[];
  imageUploadAccept: ImageUploadAccept;
}

export interface ImageUploadAccept {
  mimeTypes: string[];
  extensions: string[];
}

// 局域网内被发现的一台 AiMonitor 设备，对应 Rust 侧 DiscoveredMonitorDevice DTO
export interface DiscoveredMonitorDevice {
  // 设备唯一标识
  id: string;
  // 设备展示名称
  name: string;
  // 设备暴露的 API 版本号
  apiVersion: string;
  // 设备的基础访问地址
  baseUrl: string;
  // 设备在系统中对应的路径标识
  path: string;
  /** 设备是如何被找到的：mDNS 优先，失败回退 UDP 广播，再回退已保存地址。 */
  discoverySource: "mdns" | "udpBroadcast" | "savedAddress";
}

// 当前持久化选择的最小路由；设备离线时仍可用于展示。
export interface SavedMonitorDevice {
  id: string;
  name: string;
  baseUrl: string;
}

// Rust 在一次锁定状态中生成的设备业务快照。前端不再拼接 settings/devices。
export interface MonitorDeviceSnapshot {
  /** 后端设备状态的单调版本；较旧的异步结果不得覆盖较新的快照。 */
  revision: number;
  devices: DiscoveredMonitorDevice[];
  currentDevice: DiscoveredMonitorDevice | null;
  otherDevices: DiscoveredMonitorDevice[];
  selectedDeviceId: string;
  savedDevice: SavedMonitorDevice | null;
  hasConfiguredDevice: boolean;
  hasAvailableDevice: boolean;
}

// 设备连通性检查结果，对应 Rust 侧 ConnectionStatus DTO
export interface ConnectionStatus {
  // 是否可达
  reachable: boolean;
  // 本次检查所使用的基础地址
  baseUrl: string;
  // 附带的说明信息（成功或失败原因）
  message: string;
}

// 远程设备上的一张图片资源，对应 Rust 侧 RemoteImage DTO
export interface RemoteImage {
  // 文件名
  filename: string;
  // Rust 已验证并归一化的远端格式。
  format: ImageFormat;
  // 图片内容（通常为 base64 编码字符串）
  image: string;
}

export type ImageFormat = "jpeg" | "png" | "gif";

export interface RemoteImageCounts {
  jpeg: number;
  png: number;
  gif: number;
}

export interface RemoteImageGallery {
  images: RemoteImage[];
  counts: RemoteImageCounts;
}

// 支持接入的 AI 工具类型
export type AiTool =
  | "codex"
  | "claudeCode"
  | "cursor"
  | "openCode"
  | "workBuddy"
  | "hermes"
  | "openClaw"
  | "codeBuddy"
  | "qwenCode"
  | "kimiCode"
  | "qoder"
  | "geminiCli"
  | "gitHubCopilot";

// hook 触发时对应的行为状态
export type HookBehavior = "idle" | "running" | "asking" | "error";

// 单条 hook 上报内容，对应 Rust 侧 HookContent DTO
export interface HookContent {
  // 当前行为状态
  behavior: HookBehavior;
  // 文本内容
  content: string;
  // 附带的图片内容
  image: string;
}

// 一个 AI 工具的可编辑 Profile 草稿；设备归属由 Rust 在保存时绑定。
export interface AiProfileDraft {
  // 关联的 AI 工具类型
  tool: AiTool;
  /** 在展示屏上的显示位置，取值范围 1-25。 */
  slot: number;
  // 该工具当前的 hook 内容列表
  hooks: HookContent[];
}

// 一次读取到的完整草稿集合及其设备并发令牌。
export interface AiProfileDraftSet {
  expectedDeviceId: string;
  drafts: AiProfileDraft[];
}

// 写入 hook 配置文件后的结果，对应 Rust 侧 HookConfigWriteResult DTO
export interface HookConfigWriteResult {
  // 写入目标工具
  tool: AiTool;
  // 写入的配置文件名
  filename: string;
  // 配置实际变化后仍需执行的唯一激活流程，由对应 Rust HookProtocol 决定。
  outcome: HookWriteOutcome;
}

export type HookWriteOutcome =
  | "unchanged"
  | "active"
  | "restartRequired"
  | "codexReviewRequired"
  | "workBuddyReviewRequired"
  | "codeBuddyReviewRequired"
  | "hermesEnableRequired"
  | "openClawEnableRequired";

// hook 配置文件所在位置的信息，对应 Rust 侧 HookConfigLocation DTO
export interface HookConfigLocation {
  // 所属工具
  tool: AiTool;
  // 配置所在目录
  directory: string;
  // 配置文件完整路径
  configPath: string;
  // 是否为用户自定义目录（而非默认目录）
  isCustom: boolean;
}

// hook 中继服务的实时状态，对应 Rust 侧 HookRelayStatus DTO
export interface HookRelayStatus {
  // 中继服务是否正在监听
  listening: boolean;
  // 监听绑定的地址
  bindAddress: string;
  // 监听端口
  port: number;
  // 已接收的 hook 事件数
  receivedCount: number;
  // 已成功转发的事件数
  forwardedCount: number;
  // 转发失败的事件数
  failedCount: number;
  // 被抑制（去重/过滤）的事件数
  suppressedCount: number;
  // 当前排队等待处理的事件数
  pendingCount: number;
  // 最近一次事件的显式处理结果；释放与抑制不再通过空 behavior 推断。
  lastEvent: HookRelayLastEvent | null;
  // 最近一次错误信息
  lastError: string;
}

export type HookRelayLastEvent =
  | {
      kind: "display";
      tool: AiTool;
      hookType: string;
      behavior: HookBehavior;
    }
  | { kind: "release"; tool: AiTool; hookType: string }
  | { kind: "suppressed"; tool: AiTool; hookType: string };

// 应用运行时概览信息，对应 Rust 侧 RuntimeOverview DTO
export interface RuntimeOverview {
  // 是否已启用开机自启
  autostartEnabled: boolean;
  // 当前平台是否支持“静默启动”
  silentStartSupported: boolean;
  // hook 中继服务状态
  hookRelay: HookRelayStatus;
}

// 获取当前监控设置（连接信息、设备信息等）
export function getMonitorSettings(): Promise<MonitorSettings> {
  return invokeCommand<MonitorSettings>("get_monitor_settings");
}

// 获取由 Rust 领域层声明的工具目录、范围和 Hook 行为顺序。
export function getMonitorCapabilities(): Promise<MonitorCapabilities> {
  return invokeCommand<MonitorCapabilities>("get_monitor_capabilities");
}

// 选中某台发现的设备作为当前连接目标，并持久化到设置中
export function selectMonitorDevice(
  device: DiscoveredMonitorDevice,
): Promise<MonitorDeviceSnapshot> {
  return invokeCommand<MonitorDeviceSnapshot>("select_monitor_device", {
    device,
  });
}

// 保存用户名到监控设置
export function saveMonitorUsername(username: string): Promise<MonitorSettings> {
  return invokeCommand<MonitorSettings>("save_monitor_username", { username });
}

// 保存设备发现的自动检查间隔（分钟）
export function saveDiscoveryInterval(
  minutes: number,
): Promise<MonitorSettings> {
  return invokeCommand<MonitorSettings>("save_discovery_interval", {
    minutes,
  });
}

// 保存要在监控管理与 Hooks 管理中显示的 AI 客户端
export function saveEnabledAiTools(tools: AiTool[]): Promise<MonitorSettings> {
  return invokeCommand<MonitorSettings>("save_enabled_ai_tools", { tools });
}

// 触发一次局域网内 AiMonitor 设备发现，返回原子的设备选择与在线状态快照
export function discoverMonitorDevices(): Promise<MonitorDeviceSnapshot> {
  return invokeCommand<MonitorDeviceSnapshot>("discover_monitor_devices");
}

// 检查与设备（默认当前设备，或指定 baseUrl）的连通性
export function checkMonitorConnection(
  baseUrl?: string,
): Promise<ConnectionStatus> {
  return invokeCommand<ConnectionStatus>("check_monitor_connection", {
    baseUrl,
  });
}

// 拉取远程设备上已保存的图片列表
export function listRemoteImages(
  expectedDeviceId: string,
): Promise<RemoteImageGallery> {
  return invokeCommand<RemoteImageGallery>("list_remote_images", {
    expectedDeviceId,
  });
}

// 批量上传图片文件到远程设备
export async function uploadRemoteImages(
  files: File[],
  expectedDeviceId: string,
): Promise<string[]> {
  // 并发地把每个 File 转成 { filename, mimeType, bytes } 结构，供 Rust 侧接收
  const images = await Promise.all(
    files.map(async (file) => ({
      // 保留原始文件名
      filename: file.name,
      // 保留原始 MIME 类型
      mimeType: file.type,
      // 把文件内容读成 ArrayBuffer 再转成普通数组，便于序列化传输
      bytes: Array.from(new Uint8Array(await file.arrayBuffer())),
    })),
  );

  // 调用后端命令完成上传，返回上传成功的文件名列表
  return invokeCommand<string[]>("upload_remote_images", {
    images,
    expectedDeviceId,
  });
}

// 删除远程设备上的一张图片
export function deleteRemoteImage(
  filename: string,
  expectedDeviceId: string,
): Promise<void> {
  return invokeCommand<void>("delete_remote_image", {
    filename,
    expectedDeviceId,
  });
}

// 拉取当前设备下所有 AI 工具的 Profile 列表
export function listAiProfileDrafts(): Promise<AiProfileDraftSet> {
  return invokeCommand<AiProfileDraftSet>("list_ai_profiles");
}

// 拉取各 AI 工具的 hook 配置文件位置信息
export function listHookConfigLocations(): Promise<HookConfigLocation[]> {
  return invokeCommand<HookConfigLocation[]>("list_hook_config_locations");
}

// 保存某个工具的 hook 配置文件所在目录
export function saveHookConfigDirectory(
  tool: AiTool,
  directory: string,
): Promise<HookConfigLocation> {
  return invokeCommand<HookConfigLocation>("save_hook_config_directory", {
    tool,
    directory,
  });
}

// 弹出系统目录选择框，让用户为某工具挑选 hook 配置目录；取消时返回 null
export async function chooseHookConfigDirectory(
  defaultPath: string,
  title: string,
): Promise<string | null> {
  return open({
    // 弹窗标题
    title,
    // 只允许选择目录
    directory: true,
    // 不允许多选
    multiple: false,
    // 弹窗默认打开的路径
    defaultPath,
  });
}

// 保存（新增或更新）一个 AI 工具的 Profile
export function saveAiProfileDraft(
  profile: AiProfileDraft,
  expectedDeviceId: string,
): Promise<AiProfileDraft> {
  return invokeCommand<AiProfileDraft>("save_ai_profile", {
    profile,
    expectedDeviceId,
  });
}

// 将当前 Profile 写入指定工具的 hook 配置文件
export function writeHookConfig(
  tool: AiTool,
): Promise<HookConfigWriteResult> {
  return invokeCommand<HookConfigWriteResult>("write_hook_config", { tool });
}


// 获取运行时概览信息（自启动状态、hook 中继状态等）
export function getRuntimeOverview(): Promise<RuntimeOverview> {
  return invokeCommand<RuntimeOverview>("get_runtime_overview");
}

// 更新开机自启开关
export function updateAutostart(enabled: boolean): Promise<RuntimeOverview> {
  return invokeCommand<RuntimeOverview>("update_autostart", { enabled });
}
