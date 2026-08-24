/**
 * 持久化策略：设置经 save() 调用后端写 config/settings.json，重启保留
 */
import { defineStore } from "pinia";
import { ref, computed } from "vue";
import * as api from "@/api";
import type { AppSettings, ThemeMode, VisualMode, SidebarStyle, AIProviderConfig, MCPServerConfig } from "@/types";

/** 默认设置 — 用于后端字段缺失时填充，防止渲染崩溃 */
const defaultSettings: AppSettings = {
  data_directory: "",
  theme: "light",
  visual_mode: "standard",
  sidebar_style: "full",
  language: "zh-CN",
  user_name: "",
  show_greeting: true,
  exam_type: "",
  target_school: "",
  target_major: "",
  exam_date: "",
  target_score: 0,
  onboarding_completed: false,
  study_schedule: {
    start_time: "09:00",
    end_time: "22:00",
    daily_target_hours: 5,
    study_days_per_week: 6,
    rest_days: ["周日"],
    review_reminder_time: "23:00",
    subject_start_dates: {
      math: "",
      english: "",
      politics: "",
      professional: "",
    },
    daily_task_count: 3,
    enable_review_tasks: true,
    enable_time_tracking: false,
  },
  ai_providers: [],
  default_provider_id: "",
  mcp_servers: [],
  enabled_mcp_ids: [],
  ticktick: {
    enabled: false,
    tag_prefix: "计划",
  },
  window: {
    width: 1280,
    height: 820,
    maximized: false,
  },
  accent_color: "",
  show_logo: true,
  background_image: "",
  background_blur: 0,
  background_opacity: 1,
};

export const useSettingsStore = defineStore("settings", () => {
  const settings = ref<AppSettings | null>(null);
  const loading = ref(false);
  const error = ref<string | null>(null);

  const theme = computed<ThemeMode>(() => settings.value?.theme ?? "light");
  const visualMode = computed<VisualMode>(() => settings.value?.visual_mode ?? "standard");
  const sidebarStyle = computed<SidebarStyle>(() => settings.value?.sidebar_style ?? "full");
  const accentColor = computed<string>(() => settings.value?.accent_color ?? "");
  const showLogo = computed<boolean>(() => settings.value?.show_logo ?? true);
  const backgroundImage = computed<string>(() => settings.value?.background_image ?? "");
  const backgroundBlur = computed<number>(() => settings.value?.background_blur ?? 0);
  const backgroundOpacity = computed<number>(() => settings.value?.background_opacity ?? 1);
  // 默认 Provider 自动置顶展示，其余保持添加顺序（不修改底层数组，持久化顺序不变）
  const aiProviders = computed<AIProviderConfig[]>(() => {
    const list = settings.value?.ai_providers ?? [];
    return [...list].sort((a, b) => Number(b.is_default) - Number(a.is_default));
  });
  const mcpServers = computed<MCPServerConfig[]>(() => settings.value?.mcp_servers ?? []);
  const defaultProvider = computed(() =>
    aiProviders.value.find((p) => p.id === settings.value?.default_provider_id)
  );
  const dataDirectory = computed(() => settings.value?.data_directory ?? "");
  const onboardingCompleted = computed(() => settings.value?.onboarding_completed ?? false);
  const userName = computed(() => settings.value?.user_name ?? "");

  async function load() {
    loading.value = true;
    try {
      const raw = await api.getSettings();
      // 后端 Rust 字段名为 data_dir，前端类型为 data_directory，兼容两者
      const backendSettings = raw as AppSettings & { data_dir?: string };
      // 合并默认值，防止后端缺少字段导致渲染崩溃
      settings.value = {
        data_directory: backendSettings.data_dir || backendSettings.data_directory || defaultSettings.data_directory,
        theme: backendSettings.theme || defaultSettings.theme,
        visual_mode: backendSettings.visual_mode || defaultSettings.visual_mode,
        sidebar_style: backendSettings.sidebar_style || defaultSettings.sidebar_style,
        language: backendSettings.language || defaultSettings.language,
        user_name: backendSettings.user_name ?? defaultSettings.user_name,
        show_greeting: backendSettings.show_greeting ?? defaultSettings.show_greeting,
        exam_type: backendSettings.exam_type ?? defaultSettings.exam_type,
        target_school: backendSettings.target_school ?? defaultSettings.target_school,
        target_major: backendSettings.target_major ?? defaultSettings.target_major,
        exam_date: backendSettings.exam_date ?? defaultSettings.exam_date,
        target_score: backendSettings.target_score ?? defaultSettings.target_score,
        onboarding_completed: backendSettings.onboarding_completed ?? defaultSettings.onboarding_completed,
        study_schedule: {
          ...defaultSettings.study_schedule,
          ...(backendSettings.study_schedule || {}),
        },
        ai_providers: backendSettings.ai_providers || [],
        default_provider_id: backendSettings.default_provider_id || '',
        mcp_servers: backendSettings.mcp_servers || [],
        enabled_mcp_ids: backendSettings.enabled_mcp_ids || [],
        ticktick: backendSettings.ticktick || defaultSettings.ticktick,
        window: backendSettings.window || defaultSettings.window,
        accent_color: backendSettings.accent_color ?? defaultSettings.accent_color,
        show_logo: backendSettings.show_logo ?? defaultSettings.show_logo,
        background_image: backendSettings.background_image ?? defaultSettings.background_image,
        background_blur: backendSettings.background_blur ?? defaultSettings.background_blur,
        background_opacity: backendSettings.background_opacity ?? defaultSettings.background_opacity,
      };
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
      // 如果加载失败，使用默认设置
      settings.value = { ...defaultSettings };
    } finally {
      loading.value = false;
    }
  }

  async function save() {
    if (!settings.value) return;
    await api.saveSettings(settings.value);
  }

  /** 标记引导流程已完成并持久化 */
  async function completeOnboarding() {
    if (!settings.value) return;
    settings.value.onboarding_completed = true;
    await api.saveSettings(settings.value);
  }

  function setTheme(mode: ThemeMode) {
    if (settings.value) {
      settings.value.theme = mode;
    }
  }

  function setVisualMode(mode: VisualMode) {
    if (settings.value) {
      settings.value.visual_mode = mode;
    }
  }

  function setSidebarStyle(style: SidebarStyle) {
    if (settings.value) {
      settings.value.sidebar_style = style;
    }
  }

  function setAccentColor(color: string) {
    if (settings.value) {
      settings.value.accent_color = color;
    }
  }

  function setShowLogo(show: boolean) {
    if (settings.value) {
      settings.value.show_logo = show;
    }
  }

  function setBackgroundImage(path: string) {
    if (settings.value) {
      settings.value.background_image = path;
    }
  }

  function setBackgroundBlur(blur: number) {
    if (settings.value) {
      settings.value.background_blur = blur;
    }
  }

  function setBackgroundOpacity(opacity: number) {
    if (settings.value) {
      settings.value.background_opacity = opacity;
    }
  }

  function addProvider(provider: AIProviderConfig) {
    if (!settings.value) return;
    settings.value.ai_providers.push(provider);
    if (provider.is_default) {
      settings.value.ai_providers.forEach((p) => {
        p.is_default = p.id === provider.id;
      });
      settings.value.default_provider_id = provider.id;
    }
  }

  function updateProvider(id: string, updates: Partial<AIProviderConfig>) {
    if (!settings.value) return;
    const idx = settings.value.ai_providers.findIndex((p) => p.id === id);
    if (idx >= 0) {
      settings.value.ai_providers[idx] = { ...settings.value.ai_providers[idx], ...updates };
    }
  }

  function removeProvider(id: string) {
    if (!settings.value) return;
    settings.value.ai_providers = settings.value.ai_providers.filter((p) => p.id !== id);
    if (settings.value.default_provider_id === id) {
      settings.value.default_provider_id = settings.value.ai_providers[0]?.id ?? "";
    }
  }

  function addMCPServer(server: MCPServerConfig) {
    if (!settings.value) return;
    settings.value.mcp_servers.push(server);
  }

  function updateMCPServer(id: string, updates: Partial<MCPServerConfig>) {
    if (!settings.value) return;
    const idx = settings.value.mcp_servers.findIndex((s) => s.id === id);
    if (idx >= 0) {
      settings.value.mcp_servers[idx] = { ...settings.value.mcp_servers[idx], ...updates };
    }
  }

  function removeMCPServer(id: string) {
    if (!settings.value) return;
    settings.value.mcp_servers = settings.value.mcp_servers.filter((s) => s.id !== id);
    settings.value.enabled_mcp_ids = settings.value.enabled_mcp_ids.filter((i) => i !== id);
  }

  return {
    settings,
    loading,
    error,
    theme,
    visualMode,
    sidebarStyle,
    accentColor,
    showLogo,
    backgroundImage,
    backgroundBlur,
    backgroundOpacity,
    aiProviders,
    mcpServers,
    defaultProvider,
    dataDirectory,
    onboardingCompleted,
    userName,
    load,
    save,
    completeOnboarding,
    setTheme,
    setVisualMode,
    setSidebarStyle,
    setAccentColor,
    setShowLogo,
    setBackgroundImage,
    setBackgroundBlur,
    setBackgroundOpacity,
    addProvider,
    updateProvider,
    removeProvider,
    addMCPServer,
    updateMCPServer,
    removeMCPServer,
  };
});
