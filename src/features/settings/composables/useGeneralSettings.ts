/**
 * 设置页 — 通用设置（开机启动 + 关闭动作）
 *
 * 原 SettingsView 中 loadGeneralSettings / toggleAutostart / handleChangeCloseAction 等逻辑，
 * 仅在 Tauri 桌面环境可用（浏览器模式隐藏）。
 */
import { ref } from "vue";
import { settingsApi, type CloseAction } from "../api";

export function useGeneralSettings() {
  const autostartEnabled = ref(false);
  const autostartLoading = ref(false);
  const closeAction = ref<CloseAction>("ask");
  const closeActionLoading = ref(false);
  const isTauriEnv = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  async function loadGeneralSettings() {
    if (!isTauriEnv) return;
    try {
      const [enabled, action] = await Promise.all([
        settingsApi.getAutostart().catch(() => false),
        settingsApi.getCloseAction().catch((): CloseAction => "ask"),
      ]);
      autostartEnabled.value = enabled;
      closeAction.value = action;
    } catch (e) {
      console.warn("[General] 加载通用设置失败:", e);
    }
  }

  async function toggleAutostart(value: boolean) {
    if (autostartLoading.value) return;
    autostartLoading.value = true;
    try {
      await settingsApi.setAutostart(value);
      autostartEnabled.value = value;
    } catch (e) {
      console.error("[General] 切换开机启动失败:", e);
      // 回滚 UI
      autostartEnabled.value = !value;
    } finally {
      autostartLoading.value = false;
    }
  }

  async function handleChangeCloseAction(action: CloseAction) {
    if (closeActionLoading.value) return;
    closeActionLoading.value = true;
    try {
      await settingsApi.setCloseAction(action);
      closeAction.value = action;
    } catch (e) {
      console.error("[General] 切换关闭动作失败:", e);
    } finally {
      closeActionLoading.value = false;
    }
  }

  return {
    autostartEnabled,
    autostartLoading,
    closeAction,
    closeActionLoading,
    isTauriEnv,
    loadGeneralSettings,
    toggleAutostart,
    handleChangeCloseAction,
  };
}
