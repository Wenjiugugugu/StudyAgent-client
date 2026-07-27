/**
 * StudyAgent — Tauri Invoke Wrapper
 * 提供 Tauri/Mock 双模式：Tauri 环境调用 Rust 后端，浏览器环境回退 Mock 数据
 */

import type { ApiResponse } from "@/types";

/**
 * 检测是否运行在 Tauri 环境中
 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * 调用 Tauri 后端命令
 * @param command 命令名称
 * @param args 参数对象
 * @returns 命令返回值
 */
export async function invoke<T = unknown>(
  command: string,
  args?: Record<string, unknown>
): Promise<T> {
  if (!isTauri()) {
    throw new Error(
      `[Tauri] Command "${command}" not available in browser mode. ` +
        "Running with Mock data fallback."
    );
  }

  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, args);
}

/**
 * 带回退的调用：Tauri 优先，失败则使用 mock 函数
 */
export async function invokeWithFallback<T = unknown>(
  command: string,
  args: Record<string, unknown> | undefined,
  fallback: () => Promise<T>
): Promise<T> {
  if (!isTauri()) {
    return fallback();
  }
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    console.warn(
      `[API] Tauri command "${command}" failed, falling back to mock:`,
      error
    );
    return fallback();
  }
}

/**
 * 直接调用 Tauri 命令，不使用 Mock 回退。
 * 用于必须获取真实后端结果的命令（如 chat、testAIProvider）。
 * 在非 Tauri 环境下抛出错误。
 */
export async function invokeDirect<T>(
  command: string,
  args?: Record<string, unknown>
): Promise<T> {
  if (!isTauri()) {
    throw new Error(`命令 "${command}" 需要 Tauri 运行环境，请在桌面应用中运行`);
  }
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, args);
}

/**
 * 包装为统一 ApiResponse 格式
 */
export async function invokeApi<T = unknown>(
  command: string,
  args?: Record<string, unknown>
): Promise<ApiResponse<T>> {
  try {
    const data = await invoke<T>(command, args);
    return { success: true, data };
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    return { success: false, error: message };
  }
}
