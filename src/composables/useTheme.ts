import { watch, onMounted } from "vue";
import { useSettingsStore } from "@/stores/settings";
import type { ThemeMode, VisualMode } from "@/types";

/**
 * 主题与视觉模式管理组合式函数
 * 主题：light / dark / system
 * 视觉模式：standard / liquid-glass
 */
export function useTheme() {
  const settingsStore = useSettingsStore();

  function getSystemTheme(): "light" | "dark" {
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }

  function applyTheme(mode: ThemeMode) {
    const resolved = mode === "system" ? getSystemTheme() : mode;
    document.documentElement.setAttribute("data-theme", resolved);
  }

  function applyVisualMode(mode: VisualMode) {
    if (mode === "liquid-glass") {
      document.documentElement.setAttribute("data-visual-mode", "liquid-glass");
    } else {
      // 标准模式：移除属性，确保无 backdrop-filter 开销
      document.documentElement.removeAttribute("data-visual-mode");
    }
  }

  function toggleTheme() {
    const current = settingsStore.theme;
    const next: ThemeMode = current === "light" ? "dark" : "light";
    settingsStore.setTheme(next);
    applyTheme(next);
    settingsStore.save();
  }

  function setVisualMode(mode: VisualMode) {
    settingsStore.setVisualMode(mode);
    applyVisualMode(mode);
    settingsStore.save();
  }

  onMounted(() => {
    applyTheme(settingsStore.theme);
    applyVisualMode(settingsStore.visualMode);

    // 监听系统主题变化
    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    mediaQuery.addEventListener("change", () => {
      if (settingsStore.theme === "system") {
        applyTheme("system");
      }
    });
  });

  // 监听 store 中主题变化
  watch(
    () => settingsStore.theme,
    (newTheme) => applyTheme(newTheme)
  );

  // 监听 store 中视觉模式变化
  watch(
    () => settingsStore.visualMode,
    (newMode) => applyVisualMode(newMode)
  );

  return {
    theme: settingsStore.theme,
    visualMode: settingsStore.visualMode,
    toggleTheme,
    setVisualMode,
    setTheme: (mode: ThemeMode) => {
      settingsStore.setTheme(mode);
      applyTheme(mode);
      settingsStore.save();
    },
  };
}
