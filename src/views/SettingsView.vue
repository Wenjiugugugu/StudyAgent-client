<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, computed, watch } from "vue";
import { useRoute } from "vue-router";
import { useSettingsStore } from "@/stores/settings";
import { useUpdateStore } from "@/stores/update";
import { useTheme } from "@/composables/useTheme";
import * as api from "@/api";
import type { StudyState, SubjectState } from "@/types/state";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import DatePicker from "@/components/ui/DatePicker.vue";
import TimePicker from "@/components/ui/TimePicker.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import MarkdownText from "@/components/MarkdownText.vue";
import {
  Bot,
  Plus,
  Trash2,
  Save,
  Check,
  Eye,
  EyeOff,
  Server,
  Clock,
  Palette,
  SunMedium,
  Moon,
  Monitor,
  Zap,
  FolderOpen,
  Pencil,
  User,
  Target,
  Calendar,
  AlertCircle,
  CheckCircle,
  BookOpen,
  Gauge,
  RefreshCw,
  Download,
  Package,
  HardDriveDownload,
  Sparkles,
  Layers,
  Power,
  Minimize2,
  HelpCircle,
  PowerOff,
} from "lucide-vue-next";
import type {
  AIProviderConfig,
  MCPServerConfig,
  ProviderType,
  MCPServerType,
  ThemeMode,
  VisualMode,
} from "@/types";

const settingsStore = useSettingsStore();
const updateStore = useUpdateStore();
const route = useRoute();

const weekdayOptions = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];

// ── 本地缓冲表单（避免即时生效，点保存才提交） ──
// 结构与 settingsStore.settings 同构，但所有 v-model 绑定到此对象
const form = ref<{
  user_name: string;
  show_greeting: boolean;
  target_score: number;
  exam_date: string;
  start_time: string;
  end_time: string;
  daily_target_hours: number;
  review_reminder_time: string;
  rest_days: string[];
  daily_task_count: number;
  enable_review_tasks: boolean;
  subject_start_dates: { math: string; english: string; politics: string; professional: string };
} | null>(null);

// 教材表单（独立保存，不走 settingsStore）
const textbookForm = ref<{
  math: string;
  english: string;
  politics: string;
  professional: string;
}>({ math: "", english: "", politics: "", professional: "" });

const studyState = ref<StudyState | null>(null);
const stateLoading = ref(false);
const stateError = ref<string | null>(null);

// 各科是否活跃（用于决定教材输入框是否显示）
const subjectActive = computed(() => {
  const s = studyState.value?.subjects;
  return {
    math: s?.math?.active ?? false,
    english: s?.english?.active ?? false,
    politics: s?.politics?.active ?? false,
    professional: s?.professional?.active ?? false,
  };
});

const professionalName = computed(() => studyState.value?.subjects?.professional?.name || "专业课");

// ── rest_days 切换 ──
function toggleRestDay(day: string) {
  if (!form.value) return;
  const days = form.value.rest_days ?? [];
  if (days.includes(day)) {
    form.value.rest_days = days.filter((d) => d !== day);
  } else {
    form.value.rest_days = [...days, day];
  }
}

const computedStudyDays = computed(() => {
  if (!form.value) return 6;
  return 7 - (form.value.rest_days ?? []).length;
});

const isStudyDaysValid = computed(() => computedStudyDays.value >= 1);

// ── 快速导航 ──
interface NavSection {
  id: string;
  label: string;
  icon: any;
}

const navSections: NavSection[] = [
  { id: "personal", label: "个人信息", icon: User },
  { id: "general", label: "通用", icon: PowerOff },
  { id: "appearance", label: "外观", icon: Palette },
  { id: "goals", label: "学习目标", icon: Target },
  { id: "schedule", label: "学习时间", icon: Clock },
  { id: "rhythm", label: "学习节奏", icon: Gauge },
  { id: "textbooks", label: "教材", icon: BookOpen },
  { id: "ai-provider", label: "AI Provider", icon: Bot },
  { id: "mcp-server", label: "MCP Server", icon: Server },
  { id: "storage", label: "存储", icon: FolderOpen },
  { id: "update", label: "检查更新", icon: RefreshCw },
];

const activeSection = ref("personal");

function scrollToSection(id: string) {
  const el = document.getElementById(`settings-${id}`);
  if (el) {
    el.scrollIntoView({ behavior: "smooth", block: "start" });
    activeSection.value = id;
  }
}

function onSectionIntersect(entries: IntersectionObserverEntry[]) {
  for (const entry of entries) {
    if (entry.isIntersecting) {
      const id = entry.target.id.replace("settings-", "");
      activeSection.value = id;
    }
  }
}

let sectionObserver: IntersectionObserver | null = null;

function initSectionObserver() {
  if (sectionObserver) return;
  sectionObserver = new IntersectionObserver(onSectionIntersect, {
    rootMargin: "-20% 0px -60% 0px",
    threshold: 0,
  });
  navSections.forEach((s) => {
    const el = document.getElementById(`settings-${s.id}`);
    if (el) sectionObserver?.observe(el);
  });
}

const { setTheme, setVisualMode } = useTheme();

// ── 加载状态 ──
const saving = ref(false);
const savedFlash = ref(false);

// ── 数据目录切换 ──
const changingDir = ref(false);
const dirChangeMsg = ref<string | null>(null);
const dirChangeError = ref(false);

async function handleChangeDataDir() {
  dirChangeMsg.value = null;
  dirChangeError.value = false;

  let selected: string | null = null;
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const result = await open({ directory: true, multiple: false });
    selected = typeof result === "string" ? result : null;
  } catch (e) {
    dirChangeMsg.value = `打开目录对话框失败：${e instanceof Error ? e.message : String(e)}`;
    dirChangeError.value = true;
    return;
  }

  if (!selected) return;

  changingDir.value = true;
  try {
    const msg = await api.changeDataDirectory(selected);
    dirChangeMsg.value = msg;
    dirChangeError.value = false;
    await settingsStore.load();
    syncFormFromStore();
  } catch (e) {
    dirChangeMsg.value = e instanceof Error ? e.message : String(e);
    dirChangeError.value = true;
  } finally {
    changingDir.value = false;
  }
}

// ── Provider 表单状态 ──
const showProviderForm = ref(false);
const editingProviderId = ref<string | null>(null);
const showApiKey = ref(false);
const testing = ref(false);
const testResult = ref<string | null>(null);

function emptyProvider(): AIProviderConfig {
  return {
    id: "",
    name: "",
    type: "openai",
    base_url: "",
    api_key: "",
    model: "",
    temperature: 0.7,
    max_tokens: 4096,
    enabled: true,
    is_default: false,
  };
}

const providerForm = ref<AIProviderConfig>(emptyProvider());

const providerTypeOptions: { value: ProviderType; label: string }[] = [
  { value: "openai", label: "OpenAI" },
  { value: "gemini", label: "Gemini" },
  { value: "anthropic", label: "Anthropic" },
  { value: "ollama", label: "Ollama (本地)" },
  { value: "openrouter", label: "OpenRouter" },
  { value: "siliconflow", label: "硅基流动" },
  { value: "dashscope", label: "通义千问" },
  { value: "volcengine", label: "火山引擎" },
  { value: "custom", label: "自定义" },
];

function startAddProvider() {
  editingProviderId.value = null;
  providerForm.value = emptyProvider();
  testResult.value = null;
  showProviderForm.value = true;
}

function editProvider(p: AIProviderConfig) {
  editingProviderId.value = p.id;
  providerForm.value = { ...p };
  testResult.value = null;
  showProviderForm.value = true;
}

function cancelProviderForm() {
  showProviderForm.value = false;
  editingProviderId.value = null;
  testResult.value = null;
}

async function saveProvider() {
  if (!providerForm.value.name.trim()) return;
  if (editingProviderId.value) {
    settingsStore.updateProvider(editingProviderId.value, { ...providerForm.value });
  } else {
    settingsStore.addProvider({
      ...providerForm.value,
      id: `provider-${Date.now()}`,
    });
  }
  showProviderForm.value = false;
  await settingsStore.save();
  editingProviderId.value = null;
  testResult.value = null;
}

function removeProvider(id: string) {
  settingsStore.removeProvider(id);
}

function setDefaultProvider(id: string) {
  const s = settingsStore.settings;
  if (!s) return;
  s.ai_providers.forEach((p) => {
    p.is_default = p.id === id;
  });
  s.default_provider_id = id;
}

async function handleTestProvider() {
  testing.value = true;
  testResult.value = null;
  try {
    const result = await api.testAIProvider(providerForm.value);
    testResult.value = result.success ? (result.message || "连接成功") : `测试失败：${result.message}`;
  } catch (e) {
    testResult.value = e instanceof Error ? e.message : String(e);
  } finally {
    testing.value = false;
  }
}

// ── MCP Server 表单状态 ──
const showServerForm = ref(false);
const editingServerId = ref<string | null>(null);
const serverArgsText = ref("");

function emptyServer(): MCPServerConfig {
  return {
    id: "",
    name: "",
    type: "filesystem",
    enabled: true,
    transport: "stdio",
    command: "",
    args: [],
    url: "",
  };
}

const serverForm = ref<MCPServerConfig>(emptyServer());

const mcpTypeOptions: { value: MCPServerType; label: string }[] = [
  { value: "filesystem", label: "文件系统" },
  { value: "browser", label: "浏览器" },
  { value: "obsidian", label: "Obsidian" },
  { value: "custom", label: "自定义" },
];

const transportOptions: { value: "stdio" | "sse" | "websocket"; label: string }[] = [
  { value: "stdio", label: "STDIO" },
  { value: "sse", label: "SSE" },
  { value: "websocket", label: "WebSocket" },
];

function startAddServer() {
  editingServerId.value = null;
  serverForm.value = emptyServer();
  serverArgsText.value = "";
  showServerForm.value = true;
}

function editServer(s: MCPServerConfig) {
  editingServerId.value = s.id;
  serverForm.value = { ...s, args: [...(s.args ?? [])] };
  serverArgsText.value = (s.args ?? []).join(", ");
  showServerForm.value = true;
}

function cancelServerForm() {
  showServerForm.value = false;
  editingServerId.value = null;
}

function saveServer() {
  if (!serverForm.value.name.trim()) return;
  serverForm.value.args = serverArgsText.value
    .split(",")
    .map((a) => a.trim())
    .filter(Boolean);
  if (editingServerId.value) {
    settingsStore.updateMCPServer(editingServerId.value, { ...serverForm.value });
  } else {
    settingsStore.addMCPServer({
      ...serverForm.value,
      id: `mcp-${Date.now()}`,
    });
  }
  showServerForm.value = false;
  editingServerId.value = null;
}

function removeServer(id: string) {
  settingsStore.removeMCPServer(id);
}

function toggleServerEnabled(s: MCPServerConfig) {
  settingsStore.updateMCPServer(s.id, { enabled: !s.enabled });
}

// ── 主题 ──
const themeOptions: { mode: ThemeMode; label: string; icon: any }[] = [
  { mode: "light", label: "浅色", icon: SunMedium },
  { mode: "dark", label: "深色", icon: Moon },
  { mode: "system", label: "跟随系统", icon: Monitor },
];

function handleSetTheme(mode: ThemeMode) {
  setTheme(mode);
}

// ── 视觉模式 ──
const visualModeOptions: { mode: VisualMode; label: string; desc: string; icon: any; experimental?: boolean }[] = [
  { mode: "standard", label: "标准", desc: "稳定高性能", icon: Layers },
  { mode: "liquid-glass", label: "液态玻璃", desc: "增强视觉 · 实验性功能", icon: Sparkles, experimental: true },
];

function handleSetVisualMode(mode: VisualMode) {
  setVisualMode(mode);
}

// ── 教材保存（独立保存，立即生效） ──
const textbookSaving = ref<Record<string, boolean>>({
  math: false,
  english: false,
  politics: false,
  professional: false,
});
const textbookSavedFlash = ref<Record<string, boolean>>({
  math: false,
  english: false,
  politics: false,
  professional: false,
});

async function saveTextbook(subject: "math" | "english" | "politics" | "professional") {
  textbookSaving.value[subject] = true;
  try {
    const value = textbookForm.value[subject].trim();
    await api.updateSubjectTextbook(subject, value || null);
    if (studyState.value) {
      studyState.value.subjects[subject].textbook = value || undefined;
    }
    textbookSavedFlash.value[subject] = true;
    setTimeout(() => {
      textbookSavedFlash.value[subject] = false;
    }, 1800);
  } catch (e) {
    stateError.value = e instanceof Error ? e.message : String(e);
  } finally {
    textbookSaving.value[subject] = false;
  }
}

// ── 同步 form 从 store ──
function syncFormFromStore() {
  const s = settingsStore.settings;
  if (!s) return;
  form.value = {
    user_name: s.user_name ?? "",
    show_greeting: s.show_greeting ?? true,
    target_score: s.target_score ?? 0,
    exam_date: s.exam_date ?? "",
    start_time: s.study_schedule?.start_time ?? "09:00",
    end_time: s.study_schedule?.end_time ?? "22:00",
    daily_target_hours: s.study_schedule?.daily_target_hours ?? 5,
    review_reminder_time: s.study_schedule?.review_reminder_time ?? "23:00",
    rest_days: s.study_schedule?.rest_days?.length ? [...s.study_schedule.rest_days] : ["周日"],
    daily_task_count: s.study_schedule?.daily_task_count ?? 3,
    enable_review_tasks: s.study_schedule?.enable_review_tasks ?? true,
    subject_start_dates: {
      math: s.study_schedule?.subject_start_dates?.math ?? "",
      english: s.study_schedule?.subject_start_dates?.english ?? "",
      politics: s.study_schedule?.subject_start_dates?.politics ?? "",
      professional: s.study_schedule?.subject_start_dates?.professional ?? "",
    },
  };
}

// ── 同步 form 从 state（教材） ──
function syncTextbookFormFromState() {
  const s = studyState.value?.subjects;
  if (!s) return;
  textbookForm.value = {
    math: s.math?.textbook ?? "",
    english: s.english?.textbook ?? "",
    politics: s.politics?.textbook ?? "",
    professional: s.professional?.textbook ?? "",
  };
}

// ── 加载 StudyState ──
async function loadStudyState() {
  stateLoading.value = true;
  stateError.value = null;
  try {
    studyState.value = await api.getState();
    syncTextbookFormFromState();
  } catch (e) {
    stateError.value = e instanceof Error ? e.message : String(e);
  } finally {
    stateLoading.value = false;
  }
}

// ── 保存 ──
async function handleSave() {
  if (!isStudyDaysValid.value || !form.value || !settingsStore.settings) {
    return;
  }
  saving.value = true;
  try {
    const s = settingsStore.settings;
    s.user_name = form.value.user_name;
    s.show_greeting = form.value.show_greeting;
    s.target_score = form.value.target_score;
    s.exam_date = form.value.exam_date;
    s.study_schedule = {
      ...s.study_schedule,
      start_time: form.value.start_time,
      end_time: form.value.end_time,
      daily_target_hours: form.value.daily_target_hours,
      review_reminder_time: form.value.review_reminder_time,
      rest_days: [...form.value.rest_days],
      study_days_per_week: 7 - form.value.rest_days.length,
      daily_task_count: form.value.daily_task_count,
      enable_review_tasks: form.value.enable_review_tasks,
      subject_start_dates: { ...form.value.subject_start_dates },
    };
    await settingsStore.save();
    await settingsStore.load();
    syncFormFromStore();
    savedFlash.value = true;
    setTimeout(() => {
      savedFlash.value = false;
    }, 1800);
  } finally {
    saving.value = false;
  }
}

// ── 检查更新（使用共享 update store，与首页更新弹窗状态同步） ──
const APP_VERSION = "0.2.5";

// 从 store 获取响应式状态与方法（模板中直接引用这些名称，保持兼容）
const checking = computed(() => updateStore.checking);
const updateResult = computed(() => updateStore.updateResult);
const updateError = computed(() => updateStore.updateError);
const downloadState = computed(() => updateStore.downloadState);
const downloadProgress = computed(() => updateStore.downloadProgress);
const downloadedFilePath = computed(() => updateStore.downloadedFilePath);
const downloadError = computed(() => updateStore.downloadError);
const selectedAsset = computed({
  get: () => updateStore.selectedAsset,
  set: (v) => { updateStore.selectedAsset = v; },
});
const installing = computed(() => updateStore.installing);
const preferredAsset = computed(() => updateStore.preferredAsset);

// 代理方法
const assetLabel = (kind: string) => updateStore.assetLabel(kind);
const formatSize = (bytes: number) => updateStore.formatSize(bytes);
const handleCheckUpdate = () => updateStore.checkUpdate();
const handleDownload = () => updateStore.handleDownload();
const handleInstall = () => updateStore.handleInstall();
const resetUpdate = () => updateStore.resetUpdate();

// ── 通用设置：开机启动 + 关闭动作 ──
const autostartEnabled = ref(false);
const autostartLoading = ref(false);
const closeAction = ref<api.CloseAction>("ask");
const closeActionLoading = ref(false);
const isTauriEnv = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function loadGeneralSettings() {
  if (!isTauriEnv) return;
  try {
    const [enabled, action] = await Promise.all([
      api.getAutostart().catch(() => false),
      api.getCloseAction().catch((): api.CloseAction => "ask"),
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
    await api.setAutostart(value);
    autostartEnabled.value = value;
  } catch (e) {
    console.error("[General] 切换开机启动失败:", e);
    // 回滚 UI
    autostartEnabled.value = !value;
  } finally {
    autostartLoading.value = false;
  }
}

async function handleChangeCloseAction(action: api.CloseAction) {
  if (closeActionLoading.value) return;
  closeActionLoading.value = true;
  try {
    await api.setCloseAction(action);
    closeAction.value = action;
  } catch (e) {
    console.error("[General] 切换关闭动作失败:", e);
  } finally {
    closeActionLoading.value = false;
  }
}

// ── MCP 配置示例（小贴士） ──
const showMcpTips = ref(false);

interface McpTip {
  name: string;
  desc: string;
  type: MCPServerType;
  transport: "stdio" | "sse" | "websocket";
  command: string;
  args: string;
  url: string;
}

const MCP_TIPS: McpTip[] = [
  {
    name: "文件系统",
    desc: "让 AI 读写本地目录文件（官方推荐起步）",
    type: "filesystem",
    transport: "stdio",
    command: "npx",
    args: "-y, @modelcontextprotocol/server-filesystem, .",
    url: "",
  },
  {
    name: "GitHub",
    desc: "查询仓库、Issue、PR 等",
    type: "custom",
    transport: "stdio",
    command: "npx",
    args: "-y, @modelcontextprotocol/server-github",
    url: "",
  },
  {
    name: "Fetch 网页抓取",
    desc: "抓取网页内容供 AI 阅读",
    type: "custom",
    transport: "stdio",
    command: "uvx",
    args: "mcp-server-fetch",
    url: "",
  },
];

function applyMcpTip(tip: McpTip) {
  // 跳转到 MCP Server 区块并预填表单
  scrollToSection("mcp-server");
  startAddServer();
  serverForm.value.name = tip.name;
  serverForm.value.type = tip.type;
  serverForm.value.transport = tip.transport;
  serverForm.value.command = tip.command;
  serverArgsText.value = tip.args;
  serverForm.value.url = tip.url;
}

onMounted(async () => {
  await settingsStore.load();
  syncFormFromStore();
  await loadStudyState();
  initSectionObserver();
  void loadGeneralSettings();

  // 如果 URL hash 指向 update 区块，自动滚动到"检查更新"区域并触发检查
  if (window.location.hash === "#settings-update") {
    setTimeout(() => {
      scrollToSection("update");
      handleCheckUpdate();
    }, 200);
  }
});

// 监听 hash 变化（已在 SettingsView 内时再次点击侧边栏版本号）
watch(() => route.hash, (newHash) => {
  if (newHash === "#settings-update") {
    scrollToSection("update");
    handleCheckUpdate();
  }
});</script>

<template>
  <div class="settings-view">
    <LoadingSpinner
      v-if="settingsStore.loading && !settingsStore.settings"
      :size="28"
      label="加载设置..."
    />

    <div v-else-if="settingsStore.settings && form" class="settings-container">
      <!-- 左侧快速导航栏 -->
      <nav class="settings-nav">
        <button
          v-for="s in navSections"
          :key="s.id"
          class="nav-item"
          :class="{ active: activeSection === s.id }"
          @click="scrollToSection(s.id)"
        >
          <component :is="s.icon" :size="16" />
          <span>{{ s.label }}</span>
        </button>
      </nav>

      <div class="settings-content">
      <!-- 个人信息配置区 -->
      <Card id="settings-personal" padding="lg" class="settings-section">
        <div class="section-head">
          <div class="section-title">
            <User :size="18" />
            <span>个人信息</span>
          </div>
        </div>
        <div class="form-grid">
          <div class="form-field form-field-full">
            <label class="form-label">用户称呼</label>
            <input
              v-model="form.user_name"
              type="text"
              class="form-input"
              placeholder="如：数二（用于首页问候）"
            />
          </div>
          <div class="form-field form-field-full">
            <div class="toggle-row">
              <div class="toggle-text">
                <span class="toggle-title">首页问候显示</span>
                <span class="toggle-desc">在工作台顶部显示时间问候与称呼</span>
              </div>
              <label class="toggle-switch">
                <input v-model="form.show_greeting" type="checkbox" />
                <span class="toggle-slider" />
              </label>
            </div>
          </div>
        </div>
      </Card>

      <!-- 通用配置区：开机启动 / 关闭动作 -->
      <Card id="settings-general" padding="lg" class="settings-section">
        <div class="section-head">
          <div class="section-title">
            <PowerOff :size="18" />
            <span>通用</span>
          </div>
        </div>

        <!-- 开机启动 -->
        <div class="toggle-row">
          <div class="toggle-text">
            <span class="toggle-title">开机启动</span>
            <span class="toggle-desc">登录 Windows 后自动启动 StudyAgent</span>
          </div>
          <label class="toggle-switch">
            <input
              type="checkbox"
              :checked="autostartEnabled"
              :disabled="autostartLoading || !isTauriEnv"
              @change="toggleAutostart(($event.target as HTMLInputElement).checked)"
            />
            <span class="toggle-slider" />
          </label>
        </div>

        <!-- 关闭动作 -->
        <div class="form-field">
          <label class="form-label">关闭窗口时</label>
          <div class="close-action-options">
            <button
              type="button"
              class="close-action-option"
              :class="{ active: closeAction === 'ask' }"
              :disabled="closeActionLoading || !isTauriEnv"
              @click="handleChangeCloseAction('ask')"
            >
              <HelpCircle :size="18" class="close-action-icon" />
              <div class="close-action-text">
                <span class="close-action-label">每次询问</span>
                <span class="close-action-desc">关闭时弹窗选择</span>
              </div>
              <Check v-if="closeAction === 'ask'" :size="14" class="close-action-check" />
            </button>
            <button
              type="button"
              class="close-action-option"
              :class="{ active: closeAction === 'tray' }"
              :disabled="closeActionLoading || !isTauriEnv"
              @click="handleChangeCloseAction('tray')"
            >
              <Minimize2 :size="18" class="close-action-icon" />
              <div class="close-action-text">
                <span class="close-action-label">最小化到托盘</span>
                <span class="close-action-desc">保持后台运行</span>
              </div>
              <Check v-if="closeAction === 'tray'" :size="14" class="close-action-check" />
            </button>
            <button
              type="button"
              class="close-action-option"
              :class="{ active: closeAction === 'quit' }"
              :disabled="closeActionLoading || !isTauriEnv"
              @click="handleChangeCloseAction('quit')"
            >
              <Power :size="18" class="close-action-icon" />
              <div class="close-action-text">
                <span class="close-action-label">直接退出</span>
                <span class="close-action-desc">完全关闭应用</span>
              </div>
              <Check v-if="closeAction === 'quit'" :size="14" class="close-action-check" />
            </button>
          </div>
          <p v-if="!isTauriEnv" class="field-hint">
            当前环境不支持系统设置（仅在桌面应用中可用）
          </p>
        </div>
      </Card>

      <!-- 外观配置区（紧随个人信息） -->
      <Card id="settings-appearance" padding="lg" class="settings-section">
        <div class="section-head">
          <div class="section-title">
            <Palette :size="18" />
            <span>外观</span>
          </div>
        </div>

        <div class="form-field">
          <label class="form-label">主题</label>
          <div class="theme-options">
            <button
              v-for="opt in themeOptions"
              :key="opt.mode"
              class="theme-card"
              :class="{ active: settingsStore.theme === opt.mode }"
              @click="handleSetTheme(opt.mode)"
            >
              <component :is="opt.icon" :size="20" class="theme-icon" />
              <span class="theme-label">{{ opt.label }}</span>
              <Check v-if="settingsStore.theme === opt.mode" :size="14" class="theme-check" />
            </button>
          </div>
        </div>

        <div class="form-field">
          <label class="form-label">视觉模式</label>
          <div class="visual-mode-options">
            <button
              v-for="opt in visualModeOptions"
              :key="opt.mode"
              class="visual-mode-card"
              :class="{ active: settingsStore.visualMode === opt.mode }"
              @click="handleSetVisualMode(opt.mode)"
            >
              <div class="visual-mode-header">
                <component :is="opt.icon" :size="20" class="visual-mode-icon" />
                <span class="visual-mode-label">{{ opt.label }}</span>
                <span v-if="opt.experimental" class="experimental-badge">实验性</span>
                <Check v-if="settingsStore.visualMode === opt.mode" :size="14" class="visual-mode-check" />
              </div>
              <span class="visual-mode-desc">{{ opt.desc }}</span>
            </button>
          </div>
        </div>
      </Card>

      <!-- 学习目标配置区 -->
      <Card id="settings-goals" padding="lg" class="settings-section">
        <div class="section-head">
          <div class="section-title">
            <Target :size="18" />
            <span>学习目标</span>
          </div>
        </div>
        <div class="form-grid">
          <div class="form-field">
            <label class="form-label">目标分数</label>
            <input
              v-model.number="form.target_score"
              type="number"
              min="0"
              max="500"
              class="form-input"
              placeholder="如：360"
            />
          </div>
          <div class="form-field form-field-full">
            <label class="form-label">
              <Calendar :size="12" class="label-icon" />
              考试日期
            </label>
            <DatePicker v-model="form.exam_date" :clearable="false" />
          </div>
        </div>
      </Card>

      <!-- 学习时间配置区（仅时间相关） -->
      <Card id="settings-schedule" padding="lg" class="settings-section">
        <div class="section-head">
          <div class="section-title">
            <Clock :size="18" />
            <span>学习时间</span>
          </div>
        </div>
        <div class="form-grid">
          <div class="form-field">
            <label class="form-label">每日开始时间</label>
            <TimePicker v-model="form.start_time" :minute-step="15" />
          </div>
          <div class="form-field">
            <label class="form-label">每日结束时间</label>
            <TimePicker v-model="form.end_time" :minute-step="15" />
          </div>
          <div class="form-field">
            <label class="form-label">每日目标学时</label>
            <input v-model.number="form.daily_target_hours" type="number" step="0.5" min="0" class="form-input" />
          </div>
          <div class="form-field">
            <label class="form-label">复盘提醒时间</label>
            <TimePicker v-model="form.review_reminder_time" :minute-step="15" />
          </div>
          <div class="form-field form-field-full">
            <label class="form-label">每周休息日（可多选）</label>
            <div class="option-grid">
              <button
                v-for="day in weekdayOptions"
                :key="day"
                type="button"
                class="option-chip"
                :class="{ active: form.rest_days.includes(day) }"
                @click="toggleRestDay(day)"
              >
                {{ day }}
              </button>
            </div>
            <p class="field-hint">选择休息日后，学习天数会自动调整为剩余天数。</p>
          </div>
          <div class="form-field">
            <label class="form-label">
              每周学习天数
              <span class="field-hint">（自动计算：7 - 休息天数）</span>
            </label>
            <input
              :value="computedStudyDays"
              type="number"
              min="1"
              max="7"
              class="form-input"
              :class="{ invalid: !isStudyDaysValid }"
              readonly
              tabindex="-1"
            />
            <div v-if="!isStudyDaysValid" class="error-text">
              学习天数不能为 0，请至少选择 1 天作为学习日
            </div>
          </div>
        </div>
      </Card>

      <!-- 学习节奏配置区（任务数/总结任务/各科开始日期） -->
      <Card id="settings-rhythm" padding="lg" class="settings-section">
        <div class="section-head">
          <div class="section-title">
            <Gauge :size="18" />
            <span>学习节奏</span>
          </div>
        </div>
        <div class="form-grid">
          <div class="form-field">
            <label class="form-label">
              每天安排多少任务
              <span class="field-hint">（每科约一条；未开始的科目不安排，相应减少当日任务数）</span>
            </label>
            <input
              v-model.number="form.daily_task_count"
              type="number"
              min="1"
              max="8"
              class="form-input"
            />
            <p class="field-hint">默认 3-4 个。例如政治未到开始日期时，当天不会排政治任务。</p>
          </div>
          <div class="form-field form-field-full">
            <label class="form-label">是否安排总结/复习任务</label>
            <div class="option-grid option-grid-2">
              <button
                type="button"
                class="option-chip"
                :class="{ active: form.enable_review_tasks !== false }"
                @click="form.enable_review_tasks = true"
              >
                安排（推荐）
              </button>
              <button
                type="button"
                class="option-chip"
                :class="{ active: form.enable_review_tasks === false }"
                @click="form.enable_review_tasks = false"
              >
                只推进新知识点
              </button>
            </div>
            <p class="field-hint">关闭后 AI 不会安排"回顾"/"总结"/"复习"类任务，适合希望持续向前推进的用户。</p>
          </div>
          <div class="form-field form-field-full">
            <label class="form-label">
              各科开始学习日期
              <span class="field-hint">（留空表示立即开始；未到日期前不为该科安排任务）</span>
            </label>
            <div class="subject-start-grid">
              <div v-if="subjectActive.math" class="subject-start-item">
                <span class="subject-start-label">数学</span>
                <DatePicker v-model="form.subject_start_dates.math" placeholder="立即开始" />
              </div>
              <div v-if="subjectActive.english" class="subject-start-item">
                <span class="subject-start-label">英语</span>
                <DatePicker v-model="form.subject_start_dates.english" placeholder="立即开始" />
              </div>
              <div v-if="subjectActive.politics" class="subject-start-item">
                <span class="subject-start-label">政治</span>
                <DatePicker v-model="form.subject_start_dates.politics" placeholder="立即开始" />
              </div>
              <div v-if="subjectActive.professional" class="subject-start-item">
                <span class="subject-start-label">{{ professionalName }}</span>
                <DatePicker v-model="form.subject_start_dates.professional" placeholder="立即开始" />
              </div>
            </div>
            <p class="field-hint">例如政治计划 8 月中旬开始，可将政治开始日期设为 2026-08-15，此前不会安排政治任务。</p>
          </div>
        </div>
      </Card>

      <!-- 教材配置区（独立保存） -->
      <Card id="settings-textbooks" padding="lg" class="settings-section">
        <div class="section-head">
          <div class="section-title">
            <BookOpen :size="18" />
            <span>教材</span>
          </div>
        </div>
        <p class="section-desc">
          每科教材独立保存（点击对应"保存"立即生效）。生成周计划时，AI 会联网检索教材目录，确保任务引用的章节名与小节编号与教材实际目录一致。
        </p>
        <LoadingSpinner v-if="stateLoading" :size="20" label="加载学习状态..." />
        <div v-else-if="studyState" class="textbook-list">
          <div v-if="subjectActive.math" class="textbook-row">
            <div class="textbook-info">
              <span class="textbook-label">数学</span>
              <span class="textbook-phase">{{ studyState.subjects.math.phase }}</span>
            </div>
            <div class="textbook-input-row">
              <input
                v-model="textbookForm.math"
                type="text"
                class="form-input"
                placeholder="如：张宇高数18讲（2026版）"
              />
              <Button
                variant="primary"
                size="sm"
                :loading="textbookSaving.math"
                :disabled="textbookSaving.math || textbookSavedFlash.math"
                @click="saveTextbook('math')"
              >
                <Check v-if="textbookSavedFlash.math" :size="14" />
                <Save v-else :size="14" />
                <span>{{ textbookSavedFlash.math ? '已保存' : '保存' }}</span>
              </Button>
            </div>
          </div>
          <div v-if="subjectActive.english" class="textbook-row">
            <div class="textbook-info">
              <span class="textbook-label">英语</span>
              <span class="textbook-phase">{{ studyState.subjects.english.phase }}</span>
            </div>
            <div class="textbook-input-row">
              <input
                v-model="textbookForm.english"
                type="text"
                class="form-input"
                placeholder="如：考研英语真题黄皮书"
              />
              <Button
                variant="primary"
                size="sm"
                :loading="textbookSaving.english"
                :disabled="textbookSaving.english || textbookSavedFlash.english"
                @click="saveTextbook('english')"
              >
                <Check v-if="textbookSavedFlash.english" :size="14" />
                <Save v-else :size="14" />
                <span>{{ textbookSavedFlash.english ? '已保存' : '保存' }}</span>
              </Button>
            </div>
          </div>
          <div v-if="subjectActive.politics" class="textbook-row">
            <div class="textbook-info">
              <span class="textbook-label">政治</span>
              <span class="textbook-phase">{{ studyState.subjects.politics.phase }}</span>
            </div>
            <div class="textbook-input-row">
              <input
                v-model="textbookForm.politics"
                type="text"
                class="form-input"
                placeholder="如：肖秀荣精讲精练"
              />
              <Button
                variant="primary"
                size="sm"
                :loading="textbookSaving.politics"
                :disabled="textbookSaving.politics || textbookSavedFlash.politics"
                @click="saveTextbook('politics')"
              >
                <Check v-if="textbookSavedFlash.politics" :size="14" />
                <Save v-else :size="14" />
                <span>{{ textbookSavedFlash.politics ? '已保存' : '保存' }}</span>
              </Button>
            </div>
          </div>
          <div v-if="subjectActive.professional" class="textbook-row">
            <div class="textbook-info">
              <span class="textbook-label">{{ professionalName }}</span>
              <span class="textbook-phase">{{ studyState.subjects.professional.phase }}</span>
            </div>
            <div class="textbook-input-row">
              <input
                v-model="textbookForm.professional"
                type="text"
                class="form-input"
                placeholder="如：王道408 2026版"
              />
              <Button
                variant="primary"
                size="sm"
                :loading="textbookSaving.professional"
                :disabled="textbookSaving.professional || textbookSavedFlash.professional"
                @click="saveTextbook('professional')"
              >
                <Check v-if="textbookSavedFlash.professional" :size="14" />
                <Save v-else :size="14" />
                <span>{{ textbookSavedFlash.professional ? '已保存' : '保存' }}</span>
              </Button>
            </div>
          </div>
          <div
            v-if="!subjectActive.math && !subjectActive.english && !subjectActive.politics && !subjectActive.professional"
            class="empty-inline"
          >
            尚未启用任何科目。完成首次配置后会自动激活各科。
          </div>
        </div>
        <div v-if="stateError" class="dir-change-msg error">
          <AlertCircle :size="14" />
          <span>{{ stateError }}</span>
        </div>
      </Card>

      <!-- AI Provider 配置区 -->
      <Card id="settings-ai-provider" padding="lg" class="settings-section">
        <div class="section-head">
          <div class="section-title">
            <Bot :size="18" />
            <span>AI Provider</span>
          </div>
          <Button variant="secondary" size="sm" @click="startAddProvider">
            <Plus :size="14" />
            <span>添加</span>
          </Button>
        </div>

        <!-- Provider 列表 -->
        <div class="item-list">
          <div
            v-for="provider in settingsStore.aiProviders"
            :key="provider.id"
            class="item-row"
          >
            <div class="item-info">
              <div class="item-name-row">
                <span class="item-name">{{ provider.name }}</span>
                <Badge v-if="provider.is_default" variant="success">默认</Badge>
                <Badge v-if="!provider.enabled" variant="default">已禁用</Badge>
              </div>
              <div class="item-sub">
                <span>{{ provider.type }}</span>
                <span v-if="provider.model">· {{ provider.model }}</span>
              </div>
              <div class="item-sub text-mono">{{ provider.base_url }}</div>
            </div>
            <div class="item-actions">
              <Button
                v-if="!provider.is_default"
                variant="ghost"
                size="sm"
                @click="setDefaultProvider(provider.id)"
              >
                设为默认
              </Button>
              <Button variant="ghost" size="sm" icon @click="editProvider(provider)">
                <Pencil :size="14" />
              </Button>
              <Button variant="ghost" size="sm" icon @click="removeProvider(provider.id)">
                <Trash2 :size="14" />
              </Button>
            </div>
          </div>

          <div v-if="settingsStore.aiProviders.length === 0" class="empty-inline">
            尚未配置 AI Provider，点击「添加」开始。
          </div>
        </div>

        <!-- Provider 编辑表单 -->
        <div v-if="showProviderForm" class="edit-form">
          <div class="form-title">
            {{ editingProviderId ? "编辑 Provider" : "新增 Provider" }}
          </div>
          <div class="form-grid">
            <div class="form-field">
              <label class="form-label">名称</label>
              <input v-model="providerForm.name" type="text" class="form-input" placeholder="我的 Provider" />
            </div>
            <div class="form-field">
              <label class="form-label">类型</label>
              <select v-model="providerForm.type" class="form-select">
                <option v-for="opt in providerTypeOptions" :key="opt.value" :value="opt.value">
                  {{ opt.label }}
                </option>
              </select>
            </div>
            <div class="form-field form-field-full">
              <label class="form-label">Base URL</label>
              <input v-model="providerForm.base_url" type="text" class="form-input" placeholder="https://api.openai.com/v1" />
            </div>
            <div class="form-field form-field-full">
              <label class="form-label">API Key</label>
              <div class="input-with-action">
                <input
                  v-model="providerForm.api_key"
                  :type="showApiKey ? 'text' : 'password'"
                  class="form-input"
                  placeholder="sk-..."
                />
                <button class="input-suffix-btn" type="button" @click="showApiKey = !showApiKey">
                  <component :is="showApiKey ? EyeOff : Eye" :size="15" />
                </button>
              </div>
            </div>
            <div class="form-field">
              <label class="form-label">Model</label>
              <input v-model="providerForm.model" type="text" class="form-input" placeholder="gpt-4o" />
            </div>
            <div class="form-field">
              <label class="form-label">Temperature</label>
              <input v-model.number="providerForm.temperature" type="number" step="0.1" min="0" max="2" class="form-input" />
            </div>
            <div class="form-field">
              <label class="form-label">Max Tokens</label>
              <input v-model.number="providerForm.max_tokens" type="number" min="1" class="form-input" />
            </div>
            <div class="form-field form-field-checkbox">
              <label class="checkbox-label">
                <input v-model="providerForm.is_default" type="checkbox" class="form-checkbox" />
                <span>设为默认 Provider</span>
              </label>
            </div>
          </div>

          <div v-if="testResult" class="test-result" :class="{ error: testResult.includes('失败') || testResult.includes('错误') }">
            <Zap :size="14" />
            <span>{{ testResult }}</span>
          </div>

          <div class="form-actions">
            <Button variant="secondary" size="sm" :loading="testing" @click="handleTestProvider">
              <Zap :size="14" />
              <span>测试连接</span>
            </Button>
            <div class="form-actions-right">
              <Button variant="ghost" size="sm" @click="cancelProviderForm">取消</Button>
              <Button variant="primary" size="sm" @click="saveProvider">
                <Check :size="14" />
                <span>保存</span>
              </Button>
            </div>
          </div>
        </div>
      </Card>

      <!-- MCP 配置区 -->
      <Card id="settings-mcp-server" padding="lg" class="settings-section">
        <div class="section-head">
          <div class="section-title">
            <Server :size="18" />
            <span>MCP Server</span>
          </div>
          <Button variant="secondary" size="sm" @click="startAddServer">
            <Plus :size="14" />
            <span>添加</span>
          </Button>
        </div>

        <div class="item-list">
          <div
            v-for="server in settingsStore.mcpServers"
            :key="server.id"
            class="item-row"
          >
            <div class="item-info">
              <div class="item-name-row">
                <span class="item-name">{{ server.name }}</span>
                <Badge :variant="server.enabled ? 'success' : 'default'">
                  {{ server.enabled ? "已启用" : "已禁用" }}
                </Badge>
              </div>
              <div class="item-sub">
                <span>{{ server.type }}</span>
                <span>· {{ server.transport }}</span>
                <span v-if="server.command">· {{ server.command }}</span>
              </div>
            </div>
            <div class="item-actions">
              <Button variant="ghost" size="sm" @click="toggleServerEnabled(server)">
                {{ server.enabled ? "禁用" : "启用" }}
              </Button>
              <Button variant="ghost" size="sm" icon @click="editServer(server)">
                <Pencil :size="14" />
              </Button>
              <Button variant="ghost" size="sm" icon @click="removeServer(server.id)">
                <Trash2 :size="14" />
              </Button>
            </div>
          </div>

          <div v-if="settingsStore.mcpServers.length === 0" class="empty-inline">
            尚未配置 MCP Server。
          </div>
        </div>

        <div v-if="showServerForm" class="edit-form">
          <div class="form-title">
            {{ editingServerId ? "编辑 Server" : "新增 Server" }}
          </div>
          <div class="form-grid">
            <div class="form-field">
              <label class="form-label">名称</label>
              <input v-model="serverForm.name" type="text" class="form-input" placeholder="文件系统" />
            </div>
            <div class="form-field">
              <label class="form-label">类型</label>
              <select v-model="serverForm.type" class="form-select">
                <option v-for="opt in mcpTypeOptions" :key="opt.value" :value="opt.value">
                  {{ opt.label }}
                </option>
              </select>
            </div>
            <div class="form-field">
              <label class="form-label">传输方式</label>
              <select v-model="serverForm.transport" class="form-select">
                <option v-for="opt in transportOptions" :key="opt.value" :value="opt.value">
                  {{ opt.label }}
                </option>
              </select>
            </div>
            <div class="form-field">
              <label class="form-label">命令 (stdio)</label>
              <input v-model="serverForm.command" type="text" class="form-input" placeholder="npx" />
            </div>
            <div class="form-field form-field-full">
              <label class="form-label">参数（逗号分隔）</label>
              <input v-model="serverArgsText" type="text" class="form-input" placeholder="-y, @modelcontextprotocol/server-filesystem, ." />
            </div>
            <div class="form-field form-field-full">
              <label class="form-label">URL (SSE / WebSocket)</label>
              <input v-model="serverForm.url" type="text" class="form-input" placeholder="http://localhost:3000/sse" />
            </div>
          </div>

          <div class="form-actions">
            <Button variant="ghost" size="sm" @click="cancelServerForm">取消</Button>
            <Button variant="primary" size="sm" @click="saveServer">
              <Check :size="14" />
              <span>保存</span>
            </Button>
          </div>
        </div>

        <!-- MCP 配置小贴士 -->
        <div class="mcp-tips-block">
          <button
            type="button"
            class="mcp-tips-toggle"
            :class="{ expanded: showMcpTips }"
            @click="showMcpTips = !showMcpTips"
          >
            <HelpCircle :size="14" />
            <span>不知道填什么？查看常用 MCP Server 配置示例</span>
          </button>
          <transition name="mcp-tips-fade">
            <div v-if="showMcpTips" class="mcp-tips-list">
              <div
                v-for="(tip, idx) in MCP_TIPS"
                :key="idx"
                class="mcp-tip-card"
              >
                <div class="mcp-tip-info">
                  <div class="mcp-tip-name">{{ tip.name }}</div>
                  <div class="mcp-tip-desc">{{ tip.desc }}</div>
                  <div class="mcp-tip-cmd">
                    <code>{{ tip.command }} {{ tip.args }}</code>
                  </div>
                </div>
                <Button variant="secondary" size="sm" @click="applyMcpTip(tip)">
                  <Plus :size="12" />
                  <span>使用</span>
                </Button>
              </div>
              <p class="mcp-tips-note">
                以上配置需先安装 Node.js（npx）或 Python + uvx。命令执行需要联网下载对应 MCP Server。
              </p>
            </div>
          </transition>
        </div>
      </Card>

      <!-- 存储配置区 -->
      <Card id="settings-storage" padding="lg" class="settings-section">
        <div class="section-head">
          <div class="section-title">
            <FolderOpen :size="18" />
            <span>存储</span>
          </div>
        </div>
        <div class="form-field form-field-full">
          <label class="form-label">数据目录</label>
          <div class="data-dir">
            <FolderOpen :size="15" class="dir-icon" />
            <span class="dir-path">{{ settingsStore.dataDirectory || "未设置" }}</span>
            <Button
              variant="secondary"
              size="sm"
              :loading="changingDir"
              class="dir-change-btn"
              @click="handleChangeDataDir"
            >
              <FolderOpen :size="14" />
              <span>更改目录</span>
            </Button>
          </div>
          <p class="field-hint">
            数据目录存储学习计划、复盘记录、状态文件等。更改后历史数据不会自动迁移，如需保留请手动复制
            <code class="inline-code">state/</code>、<code class="inline-code">plan/</code>、<code class="inline-code">records/</code>、<code class="inline-code">assets/</code> 等子目录到新目录。更改后立即生效，重启后保留。
          </p>
          <div v-if="dirChangeMsg" class="dir-change-msg" :class="{ error: dirChangeError }">
            <component :is="dirChangeError ? AlertCircle : CheckCircle" :size="14" />
            <span>{{ dirChangeMsg }}</span>
          </div>
        </div>
      </Card>

      <!-- 检查更新 -->
      <Card id="settings-update" padding="lg" class="settings-section">
        <div class="section-head">
          <div class="section-title">
            <RefreshCw :size="18" />
            <span>检查更新</span>
          </div>
          <div class="current-version">
            <span class="version-label-text">当前版本</span>
            <span class="version-value text-mono">{{ APP_VERSION }}</span>
          </div>
        </div>

        <!-- 初始状态：检查按钮 -->
        <div v-if="!updateResult && !checking && !updateError" class="update-idle">
          <Button
            variant="primary"
            size="md"
            :loading="checking"
            @click="handleCheckUpdate"
          >
            <RefreshCw :size="14" />
            <span>检查更新</span>
          </Button>
          <p class="field-hint">
            点击检查是否有新版本，发现新版本后可在应用内下载并安装
          </p>
        </div>

        <!-- 检查中 -->
        <div v-else-if="checking" class="update-checking">
          <RefreshCw :size="16" class="spin" />
          <span>正在检查更新...</span>
        </div>

        <!-- 检查失败 -->
        <div v-else-if="updateError" class="update-error">
          <AlertCircle :size="16" />
          <span>检查更新失败：{{ updateError }}</span>
          <Button variant="secondary" size="sm" @click="handleCheckUpdate">
            <RefreshCw :size="13" />
            <span>重试</span>
          </Button>
        </div>

        <!-- 检查结果：无更新 -->
        <div v-else-if="updateResult && !updateResult.has_update" class="update-no-update">
          <div class="update-status-row">
            <CheckCircle :size="16" class="status-icon-ok" />
            <span class="status-text">{{ updateResult.message }}</span>
          </div>
          <Button variant="secondary" size="sm" @click="handleCheckUpdate">
            <RefreshCw :size="13" />
            <span>重新检查</span>
          </Button>
        </div>

        <!-- 检查结果：有更新 -->
        <div v-else-if="updateResult && updateResult.has_update" class="update-available">
          <!-- 版本信息 -->
          <div class="update-info">
            <div class="update-info-row">
              <span class="info-label">最新版本</span>
              <span class="info-value text-mono highlight">{{ updateResult.latest_version }}</span>
            </div>
            <div v-if="updateResult.release_name" class="update-info-row">
              <span class="info-label">Release</span>
              <span class="info-value">{{ updateResult.release_name }}</span>
            </div>
            <div v-if="updateResult.published_at" class="update-info-row">
              <span class="info-label">发布时间</span>
              <span class="info-value text-mono">
                {{ updateResult.published_at.replace('T', ' ').replace('Z', ' UTC') }}
              </span>
            </div>
          </div>

          <!-- Release notes -->
          <div v-if="updateResult.release_notes" class="release-notes-block">
            <div class="release-notes-head">更新说明</div>
            <div class="release-notes-content"><MarkdownText :content="updateResult.release_notes" /></div>
          </div>

          <!-- 安装包选择 -->
          <div v-if="updateResult.assets.length > 0" class="asset-selector">
            <label class="form-label">安装包</label>
            <div class="asset-options">
              <button
                v-for="asset in updateResult.assets"
                :key="asset.download_url"
                class="asset-option"
                :class="{ active: selectedAsset?.download_url === asset.download_url }"
                @click="selectedAsset = asset"
              >
                <Package :size="14" />
                <span class="asset-name">{{ assetLabel(asset.kind) }}</span>
                <span class="asset-size">{{ formatSize(asset.size) }}</span>
              </button>
            </div>
          </div>

          <!-- 下载进度 -->
          <div v-if="downloadState === 'downloading' && downloadProgress" class="download-progress-block">
            <div class="progress-head">
              <span class="progress-label">正在下载{{ selectedAsset ? assetLabel(selectedAsset.kind) : '安装包' }}</span>
              <span class="progress-percent">{{ downloadProgress.percent.toFixed(1) }}%</span>
            </div>
            <div class="progress-bar-track">
              <div
                class="progress-bar-fill"
                :style="{ width: `${downloadProgress.percent}%` }"
              ></div>
            </div>
            <div class="progress-detail">
              <span>{{ formatSize(downloadProgress.downloaded) }}</span>
              <span v-if="downloadProgress.total > 0">/ {{ formatSize(downloadProgress.total) }}</span>
            </div>
          </div>

          <!-- 下载完成 -->
          <div v-if="downloadState === 'downloaded'" class="download-complete">
            <CheckCircle :size="16" class="status-icon-ok" />
            <span>下载完成，点击下方按钮立即安装</span>
          </div>

          <!-- 安装中 -->
          <div v-if="downloadState === 'installing'" class="installing-state">
            <RefreshCw :size="16" class="spin" />
            <span>正在启动安装程序，应用即将退出...</span>
          </div>

          <!-- 下载/安装失败 -->
          <div v-if="downloadState === 'error' && downloadError" class="download-error-msg">
            <AlertCircle :size="14" />
            <span>{{ downloadError }}</span>
          </div>

          <!-- 操作按钮 -->
          <div class="update-actions">
            <Button
              v-if="downloadState === 'idle' || downloadState === 'error'"
              variant="primary"
              size="md"
              :disabled="!selectedAsset"
              @click="handleDownload"
            >
              <Download :size="14" />
              <span>下载安装包</span>
            </Button>
            <Button
              v-if="downloadState === 'downloaded'"
              variant="primary"
              size="md"
              :loading="installing"
              @click="handleInstall"
            >
              <HardDriveDownload :size="14" />
              <span>立即安装</span>
            </Button>
            <Button
              v-if="downloadState === 'downloading'"
              variant="secondary"
              size="md"
              disabled
            >
              <span>下载中...</span>
            </Button>
            <Button variant="ghost" size="md" @click="resetUpdate">
              <span>稍后再说</span>
            </Button>
          </div>
        </div>
      </Card>

      <!-- 悬浮保存按钮 -->
      <div class="save-fab">
        <Button
          variant="primary"
          size="lg"
          :loading="saving"
          :disabled="!isStudyDaysValid || saving || savedFlash"
          class="save-btn"
          :class="{ 'saved': savedFlash }"
          @click="handleSave"
        >
          <span class="save-btn-content">
            <Check v-if="savedFlash" :size="16" class="saved-icon" />
            <Save v-else-if="!saving" :size="16" />
            <span v-if="savedFlash">已保存</span>
            <span v-else-if="!saving">保存设置</span>
            <span v-else>保存中...</span>
          </span>
        </Button>
      </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.settings-view {
  padding: var(--space-6) var(--space-8) var(--space-12);
  /* 底部预留悬浮保存按钮空间，避免遮挡最后区块的操作按钮 */
  padding-bottom: 96px;
  display: flex;
  justify-content: center;
}

.settings-container {
  width: 100%;
  max-width: 960px;
  display: flex;
  flex-direction: row;
  align-items: flex-start;
  gap: var(--space-6);
}

/* ── 左侧快速导航 ── */
.settings-nav {
  position: sticky;
  top: var(--space-4);
  z-index: 10;
  width: 180px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  padding: var(--space-2);
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-lg);
  max-height: calc(100vh - var(--space-12));
  overflow-y: auto;
  scrollbar-width: none;
}

.settings-nav::-webkit-scrollbar {
  display: none;
}

.nav-item {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-md);
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  cursor: pointer;
  transition: all var(--transition-fast);
  font-family: inherit;
  white-space: nowrap;
  text-align: left;
}

.nav-item:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.nav-item.active {
  background: var(--accent-subtle);
  color: var(--accent);
}

.settings-content {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.settings-section {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

/* ── 区块标题 ── */
.section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
}

.section-title {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.section-desc {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  margin: 0;
  line-height: var(--leading-relaxed);
}

/* ── 列表项 ── */
.item-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.item-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
}

.item-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.item-name-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.item-name {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--text-primary);
}

.item-sub {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  display: flex;
  align-items: center;
  gap: var(--space-1);
}

.text-mono {
  font-family: var(--font-mono);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.item-actions {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  flex-shrink: 0;
}

.empty-inline {
  font-size: var(--text-sm);
  color: var(--text-tertiary);
  padding: var(--space-3);
  text-align: center;
}

/* ── 编辑表单 ── */
.edit-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  padding: var(--space-4);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
  border: 1px solid var(--border-color);
}

.form-title {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.form-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-3);
}

.form-field {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.form-field-full {
  grid-column: 1 / -1;
}

.form-field-checkbox {
  justify-content: flex-end;
}

.form-label {
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
}

.label-icon {
  vertical-align: -1px;
  flex-shrink: 0;
}

.form-input,
.form-select {
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  padding: var(--space-2) var(--space-3);
  font-size: var(--text-sm);
  font-family: inherit;
  color: var(--text-primary);
  outline: none;
  transition: border-color var(--transition-fast);
  width: 100%;
}

.form-input:focus,
.form-select:focus {
  border-color: var(--accent);
}

.form-select {
  cursor: pointer;
  appearance: none;
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%23a1a1a6' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpolyline points='6 9 12 15 18 9'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right var(--space-3) center;
  padding-right: var(--space-8);
}

.input-with-action {
  position: relative;
  display: flex;
  align-items: center;
}

.input-with-action .form-input {
  padding-right: var(--space-8);
}

.input-suffix-btn {
  position: absolute;
  right: var(--space-2);
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  padding: var(--space-1);
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-xs);
  transition: color var(--transition-fast);
}

.input-suffix-btn:hover {
  color: var(--text-primary);
}

.checkbox-label {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  color: var(--text-secondary);
  cursor: pointer;
}

.form-checkbox {
  width: 16px;
  height: 16px;
  accent-color: var(--accent);
  cursor: pointer;
}

.test-result {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--color-success-subtle);
  color: var(--color-success);
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
}

.test-result.error {
  background: var(--color-danger-subtle);
  color: var(--color-danger);
}

.form-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
}

.form-actions-right {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

/* ── Toggle 开关 ── */
.toggle-switch {
  position: relative;
  display: inline-block;
  width: 40px;
  height: 24px;
  flex-shrink: 0;
}

.toggle-switch input {
  opacity: 0;
  width: 0;
  height: 0;
}

.toggle-slider {
  position: absolute;
  cursor: pointer;
  inset: 0;
  background: var(--text-quaternary);
  border-radius: var(--radius-full);
  transition: background var(--transition-fast);
}

.toggle-slider::before {
  content: "";
  position: absolute;
  height: 18px;
  width: 18px;
  left: 3px;
  bottom: 3px;
  background: white;
  border-radius: 50%;
  transition: transform var(--transition-fast);
}

.toggle-switch input:checked + .toggle-slider {
  background: var(--color-success);
}

.toggle-switch input:checked + .toggle-slider::before {
  transform: translateX(16px);
}

/* ── 开关行（带文字说明） ── */
.toggle-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
}

.toggle-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.toggle-title {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--text-primary);
}

.toggle-desc {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

/* ── 主题选择 ── */
.theme-options {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: var(--space-3);
}

.theme-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-4) var(--space-3);
  background: var(--bg-tertiary);
  border: 2px solid transparent;
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: all var(--transition-fast);
  position: relative;
  font-family: inherit;
}

.theme-card:hover {
  background: var(--sidebar-item-hover);
}

.theme-card.active {
  border-color: var(--accent);
  background: var(--accent-subtle);
}

.theme-icon {
  color: var(--text-secondary);
}

.theme-card.active .theme-icon {
  color: var(--accent);
}

.theme-label {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
}

.theme-card.active .theme-label {
  color: var(--accent);
}

.theme-check {
  position: absolute;
  top: var(--space-2);
  right: var(--space-2);
  color: var(--accent);
}

/* ── 视觉模式选择 ── */
.visual-mode-options {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-3);
}

.visual-mode-card {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: var(--space-2);
  padding: var(--space-4) var(--space-3);
  background: var(--bg-tertiary);
  border: 2px solid transparent;
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: all var(--transition-fast);
  position: relative;
  font-family: inherit;
}

.visual-mode-card:hover {
  background: var(--sidebar-item-hover);
}

.visual-mode-card.active {
  border-color: var(--accent);
  background: var(--accent-subtle);
}

.visual-mode-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.visual-mode-icon {
  color: var(--text-secondary);
}

.visual-mode-card.active .visual-mode-icon {
  color: var(--accent);
}

.visual-mode-label {
  font-size: var(--text-sm);
  font-weight: var(--font-heading);
  color: var(--text-secondary);
}

.visual-mode-card.active .visual-mode-label {
  color: var(--accent);
}

.visual-mode-desc {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-weight: var(--font-label);
}

.visual-mode-check {
  position: absolute;
  top: var(--space-2);
  right: var(--space-2);
  color: var(--accent);
}

/* ── 数据目录 ── */
.data-dir {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-sm);
  border: 1px solid var(--border-color);
  flex-wrap: wrap;
}

.dir-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.dir-path {
  font-size: var(--text-sm);
  color: var(--text-secondary);
  font-family: var(--font-mono);
  word-break: break-all;
  flex: 1;
  min-width: 0;
}

.dir-change-btn {
  margin-left: auto;
  flex-shrink: 0;
}

.inline-code {
  font-family: var(--font-mono);
  font-size: 0.85em;
  background: var(--bg-tertiary);
  padding: 1px 6px;
  border-radius: var(--radius-xs);
  border: 1px solid var(--border-color);
}

.dir-change-msg {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--color-success-subtle);
  color: var(--color-success);
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
  margin-top: var(--space-2);
  line-height: 1.5;
}

.dir-change-msg.error {
  background: var(--color-danger-subtle);
  color: var(--color-danger);
}

.field-hint {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-weight: var(--font-normal);
}

/* ── 教材列表 ── */
.textbook-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.textbook-row {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
}

.textbook-info {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.textbook-label {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.textbook-phase {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  background: var(--bg-elevated);
  padding: 1px 6px;
  border-radius: var(--radius-full);
}

.textbook-input-row {
  display: flex;
  gap: var(--space-2);
}

.textbook-input-row .form-input {
  flex: 1;
}

/* ── 悬浮保存按钮 ── */
.save-fab {
  position: fixed;
  right: var(--space-8);
  bottom: var(--space-8);
  z-index: 20;
  display: flex;
  align-items: center;
  gap: var(--space-4);
  padding: var(--space-3) var(--space-4);
  background: color-mix(in srgb, var(--bg-primary) 85%, transparent);
  backdrop-filter: blur(12px);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-lg);
}

/* 保存按钮内容：固定 min-width，避免内容切换导致按钮尺寸变化 */
.save-btn {
  min-width: 132px;
  justify-content: center;
}

.save-btn-content {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  /* 内容变化时以按钮中心对齐，避免 hover 区域偏移 */
  justify-content: center;
}

/* 保存成功时按钮样式 */
.save-btn.saved {
  background: var(--color-success, #10b981) !important;
  border-color: var(--color-success, #10b981) !important;
}

/* saved-icon 的弹跳动画只在 icon 内部缩放，不会影响按钮尺寸 */
.saved-icon {
  /* 用 transform 配合 transform-origin 限制在 icon 自身 */
  transform-box: fill-box;
  transform-origin: center;
  animation: check-pop var(--transition-bounce);
}

@keyframes check-pop {
  0% {
    transform: scale(0.6);
    opacity: 0;
  }
  60% {
    /* 收敛到 1.05 而非 1.2，避免视觉超出按钮边界 */
    transform: scale(1.05);
  }
  100% {
    transform: scale(1);
    opacity: 1;
  }
}

/* ── 选项网格 ── */
.option-grid {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: var(--space-2);
}

.option-grid.option-grid-2 {
  grid-template-columns: repeat(2, 1fr);
}

/* ── 各科开始学习日期网格 ── */
.subject-start-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-3);
  margin-top: var(--space-2);
}

.subject-start-item {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.subject-start-label {
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  color: var(--text-tertiary);
}

.option-chip {
  padding: var(--space-2) var(--space-1);
  background: var(--bg-elevated);
  border: 1.5px solid var(--border-color);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
  cursor: pointer;
  transition: all var(--transition-fast);
  font-family: inherit;
  text-align: center;
}

.option-chip:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.option-chip.active {
  border-color: var(--accent);
  background: var(--accent-subtle);
  color: var(--accent);
}

.error-text {
  color: var(--color-danger);
  font-size: var(--text-xs);
  margin-top: var(--space-1);
}

/* ── 响应式 ── */
@media (max-width: 600px) {
  .form-grid {
    grid-template-columns: 1fr;
  }

  .theme-options {
    grid-template-columns: 1fr;
  }

  .subject-start-grid {
    grid-template-columns: 1fr;
  }
}

/* ── 检查更新区块 ── */
.current-version {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-xs);
}

.version-label-text {
  color: var(--text-tertiary);
  font-weight: var(--font-medium);
}

.version-value {
  color: var(--text-primary);
  font-weight: var(--font-semibold);
  padding: 2px var(--space-2);
  background: var(--bg-tertiary);
  border-radius: var(--radius-xs);
}

.update-idle {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  align-items: flex-start;
}

.update-checking {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.update-error {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--color-danger, #ff3b30);
  font-size: var(--text-sm);
}

.update-no-update {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  flex-wrap: wrap;
}

.update-status-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.status-icon-ok {
  color: var(--color-success, #34c759);
  flex-shrink: 0;
}

.update-available {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.update-info {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  padding: var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
}

.update-info-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: var(--text-sm);
}

.info-label {
  color: var(--text-tertiary);
  font-weight: var(--font-medium);
}

.info-value {
  color: var(--text-primary);
  font-weight: var(--font-medium);
}

.info-value.highlight {
  color: var(--accent);
  font-weight: var(--font-semibold);
}

.release-notes-block {
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  overflow: hidden;
}

.release-notes-head {
  padding: var(--space-2) var(--space-3);
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--text-tertiary);
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
}

.release-notes-content {
  margin: 0;
  padding: var(--space-3);
  font-size: var(--text-sm);
  line-height: var(--leading-relaxed);
  color: var(--text-secondary);
  word-break: break-word;
  max-height: 320px;
  overflow: auto;
}

.asset-selector {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.asset-options {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.asset-option {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--bg-tertiary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  cursor: pointer;
  font-family: inherit;
  font-size: var(--text-sm);
  color: var(--text-secondary);
  transition: all var(--transition-fast);
}

.asset-option:hover {
  border-color: var(--accent);
  color: var(--text-primary);
}

.asset-option.active {
  background: var(--accent-subtle);
  border-color: var(--accent);
  color: var(--accent);
}

.asset-name {
  font-weight: var(--font-medium);
}

.asset-size {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.asset-option.active .asset-size {
  color: var(--accent);
  opacity: 0.7;
}

.download-progress-block {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
}

.progress-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: var(--text-sm);
}

.progress-label {
  color: var(--text-secondary);
  font-weight: var(--font-medium);
}

.progress-percent {
  color: var(--accent);
  font-weight: var(--font-semibold);
  font-family: var(--font-mono);
}

.progress-bar-track {
  width: 100%;
  height: 6px;
  background: var(--bg-secondary);
  border-radius: var(--radius-xs);
  overflow: hidden;
}

.progress-bar-fill {
  height: 100%;
  background: var(--accent);
  border-radius: var(--radius-xs);
  transition: width 0.2s ease;
}

.progress-detail {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-1);
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-family: var(--font-mono);
}

.download-complete {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  color: var(--color-success, #34c759);
}

.installing-state {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.download-error-msg {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  color: var(--color-danger, #ff3b30);
}

.update-actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.spin {
  animation: spin 0.9s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

/* ── 通用：关闭动作选择 ── */
.close-action-options {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.close-action-option {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3);
  background: var(--bg-tertiary);
  border: 1.5px solid transparent;
  border-radius: var(--radius-md);
  cursor: pointer;
  font-family: inherit;
  text-align: left;
  transition: all var(--transition-fast);
  color: var(--text-primary);
}

.close-action-option:hover:not(:disabled) {
  border-color: var(--accent);
  background: var(--accent-subtle);
}

.close-action-option.active {
  border-color: var(--accent);
  background: var(--accent-subtle);
}

.close-action-option:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.close-action-option .close-action-icon {
  flex-shrink: 0;
  width: 36px;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-md);
  background: var(--accent-subtle);
  color: var(--accent);
}

.close-action-option .close-action-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex: 1;
  min-width: 0;
}

.close-action-option .close-action-label {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.close-action-option .close-action-desc {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.close-action-option .close-action-check {
  color: var(--accent);
  flex-shrink: 0;
}

/* ── MCP 配置小贴士 ── */
.mcp-tips-block {
  margin-top: var(--space-4);
  border-top: 1px dashed var(--border-color, var(--border));
  padding-top: var(--space-3);
}

.mcp-tips-toggle {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: transparent;
  border: 1px solid var(--border-color, var(--border));
  border-radius: var(--radius-md);
  cursor: pointer;
  font-family: inherit;
  font-size: var(--text-sm);
  color: var(--text-secondary);
  transition: all var(--transition-fast);
}

.mcp-tips-toggle:hover,
.mcp-tips-toggle.expanded {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-subtle);
}

.mcp-tips-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  margin-top: var(--space-3);
}

.mcp-tip-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
}

.mcp-tip-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}

.mcp-tip-name {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.mcp-tip-desc {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.mcp-tip-cmd {
  margin-top: 2px;
}

.mcp-tip-cmd code {
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  color: var(--text-secondary);
  background: var(--bg-elevated, rgba(0, 0, 0, 0.04));
  padding: 2px 6px;
  border-radius: var(--radius-sm);
  word-break: break-all;
}

.mcp-tips-note {
  margin: var(--space-2) 0 0;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  line-height: var(--leading-relaxed);
}

.mcp-tips-fade-enter-active,
.mcp-tips-fade-leave-active {
  transition: opacity var(--transition-fast), max-height var(--transition-fast);
  overflow: hidden;
}

.mcp-tips-fade-enter-from,
.mcp-tips-fade-leave-to {
  opacity: 0;
  max-height: 0;
}

.mcp-tips-fade-enter-to,
.mcp-tips-fade-leave-from {
  max-height: 600px;
}
</style>
