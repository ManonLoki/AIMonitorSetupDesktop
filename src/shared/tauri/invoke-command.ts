import { invoke } from "@tauri-apps/api/core";

export async function invokeCommand<TResult>(
  command: string,
  args?: Record<string, unknown>,
): Promise<TResult> {
  try {
    return await invoke<TResult>(command, args);
  } catch (error: unknown) {
    throw error instanceof Error
      ? error
      : new Error(`Rust command "${command}" failed: ${String(error)}`);
  }
}
