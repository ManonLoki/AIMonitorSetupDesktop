// 引入 Tauri 提供的原生 invoke 函数，用于调用 Rust 端注册的命令
import { invoke } from "@tauri-apps/api/core";

// Rust 命令失败时返回的结构化错误：`code` 对应前端 i18n 词典里的翻译 key，
// `params` 是该翻译模板的插值参数。让调用方能用当前界面语言正确展示错误，
// 而不是直接把 Rust 侧固定语言的文案透传给用户。
export class RustCommandError extends Error {
  readonly code: string;
  readonly params: Record<string, string>;

  constructor(code: string, params: Record<string, string> = {}) {
    super(code);
    this.name = "RustCommandError";
    this.code = code;
    this.params = params;
  }
}

function isAppErrorPayload(
  value: unknown,
): value is { code: string; params?: Record<string, string> } {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    typeof (value as { code: unknown }).code === "string"
  );
}

// 边界职责说明：本文件是前端唯一允许直接调用 Tauri invoke 的封装层。
// 各 feature 的 api/ 目录必须通过这个 invokeCommand 函数与 Rust 后端通信，
// 组件本身不允许直接 import invoke，从而保证所有后端调用路径统一、可追踪。
export async function invokeCommand<TResult>(
  command: string, // 要调用的 Rust 命令名（需已在 lib.rs 中注册）
  args?: Record<string, unknown>, // 传给该命令的参数（可选）
): Promise<TResult> {
  try {
    // 实际发起 Tauri invoke 调用，等待 Rust 端命令返回结果
    return await invoke<TResult>(command, args);
  } catch (error: unknown) {
    // Rust 命令失败时返回 { code, params } 结构，转换成可翻译的错误对象。
    if (isAppErrorPayload(error)) {
      throw new RustCommandError(error.code, error.params ?? {});
    }
    // 其余情况：如果抛出的不是标准 Error 实例，则包装成 Error，
    // 并附带命令名，方便调用方定位是哪个 Rust 命令失败
    throw error instanceof Error
      ? error
      : new Error(`Rust command "${command}" failed: ${String(error)}`);
  }
}
