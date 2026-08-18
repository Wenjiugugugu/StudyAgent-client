import { watch, onMounted, onUnmounted } from "vue";
import { useSettingsStore } from "@/stores/settings";
import { isTauri } from "@/api/tauri";
import { readBackgroundAsDataUrl } from "@/api";
import type { ThemeMode, VisualMode } from "@/types";

/**
 * 主题与视觉模式管理组合式函数
 * 主题：light / dark / system
 * 视觉模式：standard / liquid-glass
 * 背景图：用户上传的自定义背景图，支持模糊度与不透明度调整
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
      // 标准模式：移除属性，确保无额外样式开销
      document.documentElement.removeAttribute("data-visual-mode");
    }
  }

  /**
   * 应用自定义背景图。
   * - relativePath 为空时，清除背景图相关 CSS 变量
   * - 否则通过后端命令读取图片为 base64 data URL，注入到 --app-background-image
   * - 同时应用模糊度（--app-background-blur）与不透明度（--app-background-opacity）
   */
  async function applyBackgroundImage(
    relativePath: string,
    blur: number,
    opacity: number
  ) {
    const root = document.documentElement;
    if (!relativePath) {
      root.style.removeProperty("--app-background-image");
      root.style.removeProperty("--app-background-blur");
      root.style.removeProperty("--app-background-opacity");
      root.removeAttribute("data-has-background");
      return;
    }

    root.style.setProperty("--app-background-blur", `${blur}px`);
    root.style.setProperty("--app-background-opacity", String(opacity));
    // 标记有背景图，用于 CSS 选择器让内容区半透明
    root.setAttribute("data-has-background", "true");

    if (!isTauri()) {
      // 浏览器开发模式下无法访问 data_dir，仅写入占位
      root.style.setProperty("--app-background-image", "none");
      return;
    }

    try {
      const dataUrl = await readBackgroundAsDataUrl(relativePath);
      root.style.setProperty("--app-background-image", `url("${dataUrl}")`);
    } catch (e) {
      console.warn("[Background] 加载背景图失败:", e);
      root.style.setProperty("--app-background-image", "none");
    }
  }

  /**
   * 将 hex 颜色解析为 { r, g, b }（输入形如 "#5b8def" 或 "5b8def"）
   * 解析失败时返回 null
   */
  function parseHex(hex: string): { r: number; g: number; b: number } | null {
    const m = hex.trim().replace(/^#/, "");
    if (m.length !== 6) return null;
    const r = parseInt(m.slice(0, 2), 16);
    const g = parseInt(m.slice(2, 4), 16);
    const b = parseInt(m.slice(4, 6), 16);
    if ([r, g, b].some(Number.isNaN)) return null;
    return { r, g, b };
  }

  /** 将 rgb 转 hsl（h: 0-360, s/l: 0-1） */
  function rgbToHsl(r: number, g: number, b: number): { h: number; s: number; l: number } {
    const rr = r / 255, gg = g / 255, bb = b / 255;
    const max = Math.max(rr, gg, bb), min = Math.min(rr, gg, bb);
    let h = 0, s = 0;
    const l = (max + min) / 2;
    if (max !== min) {
      const d = max - min;
      s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
      switch (max) {
        case rr: h = (gg - bb) / d + (gg < bb ? 6 : 0); break;
        case gg: h = (bb - rr) / d + 2; break;
        case bb: h = (rr - gg) / d + 4; break;
      }
      h *= 60;
    }
    return { h, s, l };
  }

  /**
   * 应用自定义主色调。
   * 传入空字符串时清除内联样式，回退到 CSS 默认值。
   * 自动派生 hover/pressed/subtle/soft 四个变量。
   */
  function applyAccentColor(color: string) {
    const root = document.documentElement;
    if (!color || !color.trim()) {
      // 清除自定义主色调，回退到 variables.css 中的默认值
      root.style.removeProperty("--accent");
      root.style.removeProperty("--accent-hover");
      root.style.removeProperty("--accent-pressed");
      root.style.removeProperty("--accent-subtle");
      root.style.removeProperty("--accent-soft");
      return;
    }

    const rgb = parseHex(color);
    if (!rgb) return;
    const { r, g, b } = rgb;
    const { h, s, l } = rgbToHsl(r, g, b);

    // hover：略深（亮度降 8%）
    const hoverL = Math.max(0, l - 0.08);
    // pressed：更深（亮度降 16%）
    const pressedL = Math.max(0, l - 0.16);

    root.style.setProperty("--accent", color);
    root.style.setProperty("--accent-hover", `hsl(${h.toFixed(0)}, ${(s * 100).toFixed(0)}%, ${(hoverL * 100).toFixed(0)}%)`);
    root.style.setProperty("--accent-pressed", `hsl(${h.toFixed(0)}, ${(s * 100).toFixed(0)}%, ${(pressedL * 100).toFixed(0)}%)`);
    root.style.setProperty("--accent-subtle", `rgba(${r}, ${g}, ${b}, 0.1)`);
    // accent-soft：极浅的同色背景
    root.style.setProperty("--accent-soft", `hsl(${h.toFixed(0)}, ${(s * 100).toFixed(0)}%, 95%)`);
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

  // 系统主题变化监听：保存引用以便卸载时移除，避免 HMR 或多次调用泄漏监听器
  let mediaQuery: MediaQueryList | null = null;
  function onSystemThemeChange() {
    if (settingsStore.theme === "system") {
      applyTheme("system");
    }
  }

  onMounted(() => {
    applyTheme(settingsStore.theme);
    applyVisualMode(settingsStore.visualMode);
    applyAccentColor(settingsStore.accentColor);
    void applyBackgroundImage(
      settingsStore.backgroundImage,
      settingsStore.backgroundBlur,
      settingsStore.backgroundOpacity
    );

    // 监听系统主题变化
    mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    mediaQuery.addEventListener("change", onSystemThemeChange);
  });

  onUnmounted(() => {
    if (mediaQuery) {
      mediaQuery.removeEventListener("change", onSystemThemeChange);
    }
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

  // 监听自定义主色调变化
  watch(
    () => settingsStore.accentColor,
    (newColor) => applyAccentColor(newColor)
  );

  // 监听背景图/模糊度/不透明度变化
  watch(
    [
      () => settingsStore.backgroundImage,
      () => settingsStore.backgroundBlur,
      () => settingsStore.backgroundOpacity,
    ],
    ([path, blur, opacity]) => {
      void applyBackgroundImage(path, blur, opacity);
    }
  );

  return {
    theme: settingsStore.theme,
    visualMode: settingsStore.visualMode,
    accentColor: settingsStore.accentColor,
    backgroundImage: settingsStore.backgroundImage,
    backgroundBlur: settingsStore.backgroundBlur,
    backgroundOpacity: settingsStore.backgroundOpacity,
    toggleTheme,
    setVisualMode,
    setAccentColor: (color: string) => {
      settingsStore.setAccentColor(color);
      applyAccentColor(color);
      settingsStore.save();
    },
    setTheme: (mode: ThemeMode) => {
      settingsStore.setTheme(mode);
      applyTheme(mode);
      settingsStore.save();
    },
    setBackgroundImage: (path: string) => {
      settingsStore.setBackgroundImage(path);
      void applyBackgroundImage(
        path,
        settingsStore.backgroundBlur,
        settingsStore.backgroundOpacity
      );
      settingsStore.save();
    },
    setBackgroundBlur: (blur: number) => {
      settingsStore.setBackgroundBlur(blur);
      void applyBackgroundImage(
        settingsStore.backgroundImage,
        blur,
        settingsStore.backgroundOpacity
      );
      settingsStore.save();
    },
    setBackgroundOpacity: (opacity: number) => {
      settingsStore.setBackgroundOpacity(opacity);
      void applyBackgroundImage(
        settingsStore.backgroundImage,
        settingsStore.backgroundBlur,
        opacity
      );
      settingsStore.save();
    },
  };
}
