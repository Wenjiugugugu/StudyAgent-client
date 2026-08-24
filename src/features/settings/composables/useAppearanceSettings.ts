/**
 * 设置页 — 外观设置（主题 / 视觉模式 / 主色调 / Logo / 背景图）
 *
 * 原 SettingsView 中 themeOptions / visualModeOptions / accentPresets / handleSetShowLogo
 * 以及背景图上传、清除、模糊度、不透明度相关逻辑。
 */
import { ref } from "vue";
import type { Component } from "vue";
import { SunMedium, Moon, Monitor, Layers, Sparkles } from "lucide-vue-next";
import { useSettingsStore } from "@/stores/settings";
import { useTheme } from "@/composables/useTheme";
import { settingsApi } from "../api";
import type { ThemeMode, VisualMode } from "@/types";

export function useAppearanceSettings() {
  const settingsStore = useSettingsStore();
  const { setTheme, setVisualMode, setAccentColor, setBackgroundImage, setBackgroundBlur, setBackgroundOpacity } = useTheme();

  // ── 主题 ──
  const themeOptions: { mode: ThemeMode; label: string; icon: Component }[] = [
    { mode: "light", label: "浅色", icon: SunMedium },
    { mode: "dark", label: "深色", icon: Moon },
    { mode: "system", label: "跟随系统", icon: Monitor },
  ];

  function handleSetTheme(mode: ThemeMode) {
    setTheme(mode);
  }

  // ── 视觉模式 ──
  const visualModeOptions: { mode: VisualMode; label: string; desc: string; icon: Component; experimental?: boolean }[] = [
    { mode: "standard", label: "标准", desc: "稳定高性能", icon: Layers },
    { mode: "liquid-glass", label: "液态玻璃", desc: "增强视觉 · 实验性功能", icon: Sparkles, experimental: true },
  ];

  function handleSetVisualMode(mode: VisualMode) {
    setVisualMode(mode);
  }

  // ── 主色调 ──
  const accentPresets: { value: string; label: string }[] = [
    { value: "#5b8def", label: "默认蓝" },
    { value: "#6366f1", label: "靛蓝" },
    { value: "#8b5cf6", label: "紫色" },
    { value: "#ec4899", label: "粉色" },
    { value: "#ef4444", label: "红色" },
    { value: "#f59e0b", label: "橙色" },
    { value: "#10b981", label: "绿色" },
    { value: "#14b8a6", label: "青色" },
    { value: "#0ea5e9", label: "天蓝" },
  ];

  function handleSetAccentColor(color: string) {
    setAccentColor(color);
  }

  // ── Logo 显示开关 ──
  function handleSetShowLogo(show: boolean) {
    settingsStore.setShowLogo(show);
    settingsStore.save();
  }

  // ── 自定义背景图 ──
  const bgUploading = ref(false);
  const bgUploadError = ref<string | null>(null);

  async function handleUploadBackground() {
    bgUploadError.value = null;
    bgUploading.value = true;
    try {
      const relativePath = await settingsApi.saveBackgroundImage();
      setBackgroundImage(relativePath);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      // 用户取消选择不视为错误
      if (!msg.includes("未选择文件")) {
        bgUploadError.value = msg;
      }
    } finally {
      bgUploading.value = false;
    }
  }

  async function handleClearBackground() {
    const prevPath = settingsStore.backgroundImage;
    // 先清除设置中的路径，再删除文件
    setBackgroundImage("");
    if (prevPath) {
      try {
        await settingsApi.deleteBackgroundImage(prevPath);
      } catch (e) {
        // 文件删除失败不阻塞，仅记录日志
        console.warn("[Background] 删除背景图文件失败:", e);
      }
    }
  }

  function handleSetBackgroundBlur(blur: number) {
    setBackgroundBlur(blur);
  }

  function handleSetBackgroundOpacity(opacity: number) {
    setBackgroundOpacity(opacity);
  }

  return {
    themeOptions,
    handleSetTheme,
    visualModeOptions,
    handleSetVisualMode,
    accentPresets,
    handleSetAccentColor,
    handleSetShowLogo,
    bgUploading,
    bgUploadError,
    handleUploadBackground,
    handleClearBackground,
    handleSetBackgroundBlur,
    handleSetBackgroundOpacity,
  };
}
