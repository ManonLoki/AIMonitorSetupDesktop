// 引入统一的 Tauri invoke 封装，所有对 Rust 命令的调用都必须经过这一层
import { invokeCommand } from "../../../shared/tauri/invoke-command";

// 系统概览信息的数据结构，与 Rust 端 get_system_overview 命令返回的 DTO 对应
export interface SystemOverview {
  appName: string; // 应用名称
  version: string; // 应用版本号
  backend: string; // 后端标识（例如 Rust 后端名称）
  transport: string; // 前后端通信方式描述（例如 Tauri invoke）
}

// 调用 Rust 的 get_system_overview 命令，获取应用/后端概览信息
export function getSystemOverview(): Promise<SystemOverview> {
  return invokeCommand<SystemOverview>("get_system_overview");
}
