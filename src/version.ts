/**
 * 应用版本号 —— 统一管理入口（单一来源）
 *
 * 分层：
 * - 权威来源：Tauri 运行时 `get_app_version`（返回 tauri.conf.json / Cargo.toml 中声明的版本）。
 * - FALLBACK_APP_VERSION：仅「浏览器开发预览模式」使用的兜底版本号。
 *   这是一份唯一需要在前端手动维护的版本字符串，升级版本时只需在此同步一处。
 *
 * 使用约定：任何需要显示版本号的地方（侧边栏、调试页、设置页…）一律通过
 * useAppVersion() 读取，禁止再在组件内各自写死版本字符串。
 */

import { ref, type Ref } from "vue";
import { invokeDirect, isTauri } from "@/api/tauri";

/** 兜底版本号（浏览器模式 / 读取失败时） */
export const FALLBACK_APP_VERSION = "0.5.2";

/** 调用后端获取应用真实版本 */
async function fetchAppVersion(): Promise<string> {
  return invokeDirect<string>("get_app_version");
}

/**
 * 读取真实版本；非 Tauri 环境或读取失败时回退到 FALLBACK_APP_VERSION
 */
export async function resolveAppVersion(): Promise<string> {
  try {
    if (isTauri()) {
      const v = await fetchAppVersion();
      if (v) return v;
    }
  } catch {
    /* 忽略，回退到兜底版本 */
  }
  return FALLBACK_APP_VERSION;
}

/**
 * 响应式版本号：初始为兜底值，异步刷新为运行时真实版本。
 * 显示版本号时使用此组合式函数。
 */
export function useAppVersion(): { version: Ref<string> } {
  const version = ref<string>(FALLBACK_APP_VERSION);
  resolveAppVersion().then((v) => {
    version.value = v;
  });
  return { version };
}