<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import * as api from "@/api";
import { isTauri } from "@/api";
import { todayString, formatDateShanghai } from "@/utils/date";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import CodeBlock from "@/components/CodeBlock.vue";
import {
  RefreshCw,
  Cpu,
  FolderTree,
  Boxes,
  LayoutDashboard,
  Bot,
  Settings2,
  ScrollText,
  Zap,
  FileText,
  ChevronRight,
  ChevronDown,
  Trash2,
  Clock,
  Calendar,
  CheckCircle2,
  Target,
  Menu,
  FileCheck,
  Radio,
  Send,
  AlertTriangle,
  Coins,
} from "lucide-vue-next";
import { useAiDebugStore } from "@/stores/aiDebug";
import {
  estimateCost,
  formatCost,
  formatTokens,
  fetchLatestPricingNote,
} from "@/utils/aiPricing";
import type {
  StudyState,
  DashboardSummary,
  AppSettings,
  AIProviderConfig,
  DailyPlan,
  ReviewRecord,
  AiUsageEntry,
} from "@/types";

// ── 系统信息 ──
const APP_VERSION = "0.4.0";
const TAURI_VERSION = "2.x";
const sysInfo = computed(() => ({
  appVersion: APP_VERSION,
  tauriVersion: isTauri() ? TAURI_VERSION : "未运行（浏览器模式）",
  environment: isTauri() ? "Tauri 桌面应用" : "浏览器开发模式",
  dataDirectory: dataDir.value || "未设置",
}));
const localTime = ref("");
const utcTime = ref("");
let timeTimer: number | undefined;

// ── 快速导航 ──
interface DebugSection {
  id: string;
  label: string;
  icon: any;
}

const debugSections: DebugSection[] = [
  { id: "sysinfo", label: "系统信息", icon: Cpu },
  { id: "files", label: "数据文件", icon: FolderTree },
  { id: "state", label: "State", icon: Boxes },
  { id: "plan", label: "Plan", icon: Calendar },
  { id: "review", label: "Review", icon: FileCheck },
  { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { id: "providers", label: "AI Provider", icon: Bot },
  { id: "ai-calls", label: "AI 调用", icon: Radio },
  { id: "ai-usage", label: "AI 用量", icon: Coins },
  { id: "settings", label: "Settings", icon: Settings2 },
  { id: "logs", label: "日志", icon: ScrollText },
];

const activeSection = ref("sysinfo");

function scrollToSection(id: string) {
  const el = document.getElementById(`debug-${id}`);
  if (el) {
    el.scrollIntoView({ behavior: "smooth", block: "start" });
    activeSection.value = id;
  }
}

function onSectionIntersect(entries: IntersectionObserverEntry[]) {
  for (const entry of entries) {
    if (entry.isIntersecting) {
      const id = entry.target.id.replace("debug-", "");
      activeSection.value = id;
    }
  }
}

let sectionObserver: IntersectionObserver | null = null;

function initSectionObserver() {
  if (sectionObserver) return;
  sectionObserver = new IntersectionObserver(onSectionIntersect, {
    rootMargin: "-15% 0px -60% 0px",
    threshold: 0,
  });
  debugSections.forEach((s) => {
    const el = document.getElementById(`debug-${s.id}`);
    if (el) sectionObserver?.observe(el);
  });
}

function updateClock() {
  const now = new Date();
  localTime.value = now.toLocaleString("zh-CN", { hour12: false });
  utcTime.value = now.toISOString();
}

// ── 数据目录 ──
const dataDir = ref("");

// ── 数据文件检查 ──
interface DirEntry {
  name: string;
  path: string;
  isDirectory: boolean;
}
interface DirCheck {
  name: string;
  label: string;
  exists: boolean | null; // null = 未检测
  loading: boolean;
  error: string | null;
  entries: DirEntry[];
}
const dataDirs = ref<DirCheck[]>([
  { name: "state", label: "state（状态）", exists: null, loading: false, error: null, entries: [] },
  { name: "plan", label: "plan（计划）", exists: null, loading: false, error: null, entries: [] },
  { name: "records", label: "records（复盘记录）", exists: null, loading: false, error: null, entries: [] },
  { name: "config", label: "config（配置）", exists: null, loading: false, error: null, entries: [] },
]);
const expandedDir = ref<string | null>(null);
const fileContent = ref<{ dir: string; name: string; content: string; error: string | null } | null>(null);
const loadingFile = ref(false);

async function checkDataDirs() {
  if (!dataDir.value) return;
  for (const dir of dataDirs.value) {
    dir.loading = true;
    dir.error = null;
    try {
      const entries = await readDirSafe(joinPath(dataDir.value, dir.name));
      dir.exists = true;
      dir.entries = entries;
    } catch (e) {
      dir.exists = false;
      dir.error = e instanceof Error ? e.message : String(e);
      dir.entries = [];
    } finally {
      dir.loading = false;
    }
  }
}

function toggleDir(name: string) {
  expandedDir.value = expandedDir.value === name ? null : name;
}

async function viewFile(dirName: string, entry: DirEntry) {
  if (entry.isDirectory) return;
  loadingFile.value = true;
  fileContent.value = null;
  try {
    const content = await readFileSafe(joinPath(dataDir.value, dirName, entry.name));
    fileContent.value = { dir: dirName, name: entry.name, content, error: null };
  } catch (e) {
    fileContent.value = {
      dir: dirName,
      name: entry.name,
      content: "",
      error: e instanceof Error ? e.message : String(e),
    };
  } finally {
    loadingFile.value = false;
  }
}

// ── State 解析测试 ──
interface TestResult<T> {
  status: "idle" | "loading" | "success" | "error";
  data: T | null;
  error: string | null;
}
const stateTest = ref<TestResult<StudyState>>({ status: "idle", data: null, error: null });

async function runStateTest() {
  stateTest.value = { status: "loading", data: null, error: null };
  try {
    const data = await api.getState();
    stateTest.value = { status: "success", data, error: null };
  } catch (e) {
    stateTest.value = {
      status: "error",
      data: null,
      error: e instanceof Error ? e.message : String(e),
    };
  }
}

// ── Dashboard 数据测试 ──
const dashboardTest = ref<TestResult<DashboardSummary>>({ status: "idle", data: null, error: null });

async function runDashboardTest() {
  dashboardTest.value = { status: "loading", data: null, error: null };
  try {
    const data = await api.getDashboardSummary();
    dashboardTest.value = { status: "success", data, error: null };
  } catch (e) {
    dashboardTest.value = {
      status: "error",
      data: null,
      error: e instanceof Error ? e.message : String(e),
    };
  }
}

const dashboardAbnormal = computed(() => {
  const d = dashboardTest.value.data;
  if (!d) return [];
  const issues: string[] = [];
  if (!d.date) issues.push("date 为空");
  if (d.today_tasks && d.today_tasks.total === 0) issues.push("今日任务总数为 0");
  if (d.week_progress && d.week_progress.target_hours === 0) issues.push("周目标学时为 0");
  if (!d.subject_progress || d.subject_progress.length === 0) issues.push("科目进度为空");
  return issues;
});

// ── Plan 测试 ──
const planTest = ref<TestResult<DailyPlan>>({ status: "idle", data: null, error: null });
const planTestDate = ref(todayString());

async function runPlanTest() {
  planTest.value = { status: "loading", data: null, error: null };
  try {
    const data = await api.getPlanByDate(planTestDate.value);
    planTest.value = { status: "success", data, error: null };
  } catch (e) {
    planTest.value = {
      status: "error",
      data: null,
      error: e instanceof Error ? e.message : String(e),
    };
  }
}

const planAbnormal = computed(() => {
  const p = planTest.value.data;
  if (!p) return [];
  const issues: string[] = [];
  if (!p.meta.date) issues.push("meta.date 为空");
  if (!p.data?.tasks || p.data.tasks.length === 0) issues.push("任务列表为空");
  if (p.data?.total_tasks === 0) issues.push("total_tasks 为 0");
  return issues;
});

// ── Review 测试 ──
const reviewTest = ref<TestResult<ReviewRecord>>({ status: "idle", data: null, error: null });
const reviewTestDate = ref(todayString());

async function runReviewTest() {
  reviewTest.value = { status: "loading", data: null, error: null };
  try {
    const data = await api.getReview(reviewTestDate.value);
    reviewTest.value = { status: "success", data, error: null };
  } catch (e) {
    reviewTest.value = {
      status: "error",
      data: null,
      error: e instanceof Error ? e.message : String(e),
    };
  }
}

const reviewAbnormal = computed(() => {
  const r = reviewTest.value.data;
  if (!r) return [];
  const issues: string[] = [];
  if (!r.meta.date) issues.push("meta.date 为空");
  if (r.data?.completion.priority_a_total + r.data.completion.priority_b_total === 0) {
    issues.push("任务总数为 0");
  }
  return issues;
});

// ── AI Provider 测试 ──
interface ProviderTestState {
  provider: AIProviderConfig;
  status: "idle" | "loading" | "success" | "error";
  message: string;
}
const providerTests = ref<ProviderTestState[]>([]);

async function loadProviders() {
  try {
    const s = await api.getSettings();
    providerTests.value = (s.ai_providers ?? []).map((p) => ({
      provider: p,
      status: "idle",
      message: "",
    }));
  } catch {
    providerTests.value = [];
  }
}

async function testProvider(idx: number) {
  const item = providerTests.value[idx];
  if (!item) return;
  item.status = "loading";
  item.message = "";
  try {
    const result = await api.testAIProvider(item.provider);
    item.status = result.success ? "success" : "error";
    item.message = result.message;
  } catch (e) {
    item.status = "error";
    item.message = e instanceof Error ? e.message : String(e);
  }
}

async function testAllProviders() {
  for (let i = 0; i < providerTests.value.length; i++) {
    await testProvider(i);
  }
}

// ── AI 调用记录 ──
const aiDebugStore = useAiDebugStore();
/** 当前展开查看详情的记录 ID（null 表示全部折叠） */
const expandedAiCallId = ref<number | null>(null);

function toggleAiCall(id: number) {
  expandedAiCallId.value = expandedAiCallId.value === id ? null : id;
}

function formatDuration(ms: number | null): string {
  if (ms === null) return "—";
  if (ms < 1000) return `${ms} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

function formatTimestamp(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleTimeString("zh-CN", { hour12: false });
  } catch {
    return iso;
  }
}

function aiCallStatusBadge(status: "pending" | "success" | "error"): "info" | "success" | "danger" {
  if (status === "success") return "success";
  if (status === "error") return "danger";
  return "info";
}

function aiCallStatusLabel(status: "pending" | "success" | "error"): string {
  if (status === "success") return "成功";
  if (status === "error") return "失败";
  return "进行中";
}

// ── AI 用量日志（持久化记录，含费用估算） ──
const aiUsageLog = ref<AiUsageEntry[]>([]);
const aiUsageLoading = ref(false);
const aiUsageError = ref<string | null>(null);
const expandedUsageIdx = ref<number | null>(null);
const usageTimeFilter = ref<"all" | "today" | "7d" | "30d">("all");

/** 定价表更新提示 */
const pricingNote = fetchLatestPricingNote();

/** 按时间筛选后的用量记录（倒序：最新在前） */
const filteredUsageLog = computed<AiUsageEntry[]>(() => {
  if (usageTimeFilter.value === "all") {
    return [...aiUsageLog.value].reverse();
  }
  const now = Date.now();
  const ranges: Record<string, number> = {
    today: 24 * 60 * 60 * 1000,
    "7d": 7 * 24 * 60 * 60 * 1000,
    "30d": 30 * 24 * 60 * 60 * 1000,
  };
  const range = ranges[usageTimeFilter.value];
  return aiUsageLog.value
    .filter((e) => {
      const t = new Date(e.timestamp).getTime();
      return now - t <= range;
    })
    .reverse();
});

/** 单条记录的费用估算（缓存以避免重复计算） */
const usageCostMap = computed<Map<string, ReturnType<typeof estimateCost>>>(() => {
  const map = new Map<string, ReturnType<typeof estimateCost>>();
  for (const entry of aiUsageLog.value) {
    const key = usageEntryKey(entry);
    if (!map.has(key)) {
      map.set(
        key,
        estimateCost(entry.model, entry.prompt_tokens, entry.completion_tokens),
      );
    }
  }
  return map;
});

/** 用量汇总统计 */
const usageSummary = computed(() => {
  const log = filteredUsageLog.value;
  let totalInput = 0;
  let totalOutput = 0;
  let totalCalls = log.length;
  let successCalls = 0;
  let errorCalls = 0;
  let totalCost = 0;
  let totalDurationMs = 0;
  const byModel = new Map<string, { calls: number; input: number; output: number; cost: number }>();
  const byAgent = new Map<string, { calls: number; input: number; output: number; cost: number }>();

  for (const entry of log) {
    totalInput += entry.prompt_tokens;
    totalOutput += entry.completion_tokens;
    totalDurationMs += entry.duration_ms;
    if (entry.status === "success") successCalls++;
    if (entry.status === "error") errorCalls++;

    const cost = usageCostMap.value.get(usageEntryKey(entry))?.costCny ?? 0;
    totalCost += cost;

    const modelKey = entry.model || "(unknown)";
    const modelStat = byModel.get(modelKey) ?? { calls: 0, input: 0, output: 0, cost: 0 };
    modelStat.calls++;
    modelStat.input += entry.prompt_tokens;
    modelStat.output += entry.completion_tokens;
    modelStat.cost += cost;
    byModel.set(modelKey, modelStat);

    const agentKey = entry.agent || "unknown";
    const agentStat = byAgent.get(agentKey) ?? { calls: 0, input: 0, output: 0, cost: 0 };
    agentStat.calls++;
    agentStat.input += entry.prompt_tokens;
    agentStat.output += entry.completion_tokens;
    agentStat.cost += cost;
    byAgent.set(agentKey, agentStat);
  }

  return {
    totalCalls,
    successCalls,
    errorCalls,
    totalInput,
    totalOutput,
    totalCost,
    totalDurationMs,
    avgDurationMs: totalCalls > 0 ? Math.round(totalDurationMs / totalCalls) : 0,
    byModel: Array.from(byModel.entries())
      .map(([model, stat]) => ({ model, ...stat }))
      .sort((a, b) => b.cost - a.cost),
    byAgent: Array.from(byAgent.entries())
      .map(([agent, stat]) => ({ agent, ...stat }))
      .sort((a, b) => b.calls - a.calls),
  };
});

function usageEntryKey(entry: AiUsageEntry): string {
  return `${entry.timestamp}|${entry.model}|${entry.prompt_tokens}|${entry.completion_tokens}`;
}

function toggleUsageEntry(idx: number) {
  expandedUsageIdx.value = expandedUsageIdx.value === idx ? null : idx;
}

function formatUsageDuration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

function formatUsageTimestamp(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleString("zh-CN", { hour12: false });
  } catch {
    return iso;
  }
}

function usageStatusBadge(status: string): "success" | "danger" | "info" {
  if (status === "success") return "success";
  if (status === "error") return "danger";
  return "info";
}

function usageStatusLabel(status: string): string {
  if (status === "success") return "成功";
  if (status === "error") return "失败";
  return "未知";
}

function agentLabel(agent: string): string {
  const map: Record<string, string> = {
    planner: "计划生成",
    reviewer: "复盘",
    assistant: "助手",
    teacher: "教学",
    unknown: "未知",
  };
  return map[agent] ?? agent;
}

async function loadAiUsageLog() {
  aiUsageLoading.value = true;
  aiUsageError.value = null;
  try {
    aiUsageLog.value = await api.getAiUsageLog();
  } catch (e) {
    aiUsageError.value = e instanceof Error ? e.message : String(e);
    aiUsageLog.value = [];
  } finally {
    aiUsageLoading.value = false;
  }
}

async function clearAiUsageLog() {
  if (!confirm("确认清空全部 AI 用量日志？此操作不可恢复。")) return;
  try {
    await api.clearAiUsageLog();
    aiUsageLog.value = [];
    expandedUsageIdx.value = null;
  } catch (e) {
    aiUsageError.value = e instanceof Error ? e.message : String(e);
  }
}

// ── Settings 查看 ──
const settingsView = ref<TestResult<AppSettings>>({ status: "idle", data: null, error: null });
const configPath = computed(() => settingsView.value.data?.data_directory ?? "");

async function loadSettingsView() {
  settingsView.value = { status: "loading", data: null, error: null };
  try {
    const data = await api.getSettings();
    settingsView.value = { status: "success", data, error: null };
  } catch (e) {
    settingsView.value = {
      status: "error",
      data: null,
      error: e instanceof Error ? e.message : String(e),
    };
  }
}

// ── 日志捕获 ──
interface LogEntry {
  level: "log" | "warn" | "error" | "info";
  time: string;
  args: string;
}
const logs = ref<LogEntry[]>([]);
let originalConsole: { log: (...a: unknown[]) => void; warn: (...a: unknown[]) => void; error: (...a: unknown[]) => void; info: (...a: unknown[]) => void } | null = null;

function stringifyArgs(args: unknown[]): string {
  return args
    .map((a) => {
      if (typeof a === "string") return a;
      try {
        return JSON.stringify(a);
      } catch {
        return String(a);
      }
    })
    .join(" ");
}

function captureConsole() {
  originalConsole = {
    log: console.log.bind(console),
    warn: console.warn.bind(console),
    error: console.error.bind(console),
    info: console.info.bind(console),
  };
  const push = (level: LogEntry["level"]) => (...args: unknown[]) => {
    originalConsole?.[level](...args);
    logs.value.push({
      level,
      time: new Date().toLocaleTimeString("zh-CN", { hour12: false }),
      args: stringifyArgs(args),
    });
  };
  console.log = push("log");
  console.warn = push("warn");
  console.error = push("error");
  console.info = push("info");
}

function restoreConsole() {
  if (originalConsole) {
    console.log = originalConsole.log;
    console.warn = originalConsole.warn;
    console.error = originalConsole.error;
    console.info = originalConsole.info;
    originalConsole = null;
  }
}

function clearLogs() {
  logs.value = [];
}

// ── JSON 格式化 ──
function formatJson(obj: unknown): string {
  try {
    return JSON.stringify(obj, null, 2);
  } catch {
    return String(obj);
  }
}

// ── 文件系统辅助（仅 Tauri 环境） ──
function joinPath(...parts: string[]): string {
  return parts.filter(Boolean).join("/").replace(/\\/g, "/").replace(/\/+/g, "/");
}

async function readDirSafe(dirPath: string): Promise<DirEntry[]> {
  if (!isTauri()) {
    throw new Error("需要 Tauri 桌面环境才能读取目录");
  }
  const { readDir } = await import("@tauri-apps/plugin-fs");
  const result = await readDir(dirPath);
  return result.map((entry) => ({
    name: entry.name ?? "",
    path: joinPath(dirPath, entry.name ?? ""),
    isDirectory: !!entry.isDirectory,
  }));
}

async function readFileSafe(filePath: string): Promise<string> {
  if (!isTauri()) {
    throw new Error("需要 Tauri 桌面环境才能读取文件");
  }
  const { readTextFile } = await import("@tauri-apps/plugin-fs");
  return await readTextFile(filePath);
}

// ── 全局刷新 ──
const refreshing = ref(false);
async function refreshAll() {
  refreshing.value = true;
  try {
    await Promise.all([
      loadSettingsView().then(() => {
        dataDir.value = settingsView.value.data?.data_directory ?? "";
      }),
      loadProviders(),
      runStateTest(),
      runDashboardTest(),
      runPlanTest(),
      runReviewTest(),
      loadAiUsageLog(),
    ]);
    await checkDataDirs();
  } finally {
    refreshing.value = false;
  }
}

function statusBadge(status: TestResult<unknown>["status"] | ProviderTestState["status"]) {
  if (status === "success") return "success";
  if (status === "error") return "danger";
  if (status === "loading") return "info";
  return "default";
}

function statusLabel(status: TestResult<unknown>["status"] | ProviderTestState["status"]) {
  const map: Record<string, string> = {
    idle: "待测试",
    loading: "测试中",
    success: "成功",
    error: "失败",
  };
  return map[status] ?? status;
}

onMounted(async () => {
  updateClock();
  timeTimer = window.setInterval(updateClock, 1000);
  captureConsole();
  await refreshAll();
  initSectionObserver();
});

onUnmounted(() => {
  if (timeTimer) window.clearInterval(timeTimer);
  restoreConsole();
});
</script>

<template>
  <div class="debug-view">
    <!-- 顶部操作栏 -->
    <header class="debug-header">
      <p class="debug-desc">用于排查问题，查看系统信息、数据文件、API 响应与运行日志。</p>
      <Button variant="secondary" size="sm" :loading="refreshing" @click="refreshAll">
        <RefreshCw :size="14" />
        <span>刷新全部</span>
      </Button>
    </header>

    <div class="debug-container">
      <!-- 左侧快速导航栏 -->
      <nav class="debug-nav-side">
        <button
          v-for="s in debugSections"
          :key="s.id"
          class="nav-item"
          :class="{ active: activeSection === s.id }"
          @click="scrollToSection(s.id)"
        >
          <component :is="s.icon" :size="14" />
          <span>{{ s.label }}</span>
        </button>
      </nav>

      <div class="debug-content">
        <!-- 1. 系统信息 -->
    <Card id="debug-sysinfo" padding="lg" class="debug-section">
      <div class="section-head">
        <div class="section-title">
          <Cpu :size="18" />
          <span>系统信息</span>
        </div>
      </div>
      <div class="info-grid">
        <div class="info-row">
          <span class="info-key">应用版本</span>
          <span class="info-value text-mono">{{ sysInfo.appVersion }}</span>
        </div>
        <div class="info-row">
          <span class="info-key">Tauri 版本</span>
          <span class="info-value text-mono">{{ sysInfo.tauriVersion }}</span>
        </div>
        <div class="info-row">
          <span class="info-key">运行环境</span>
          <span class="info-value">{{ sysInfo.environment }}</span>
        </div>
        <div class="info-row">
          <span class="info-key">数据目录</span>
          <span class="info-value text-mono break-all">{{ sysInfo.dataDirectory }}</span>
        </div>
        <div class="info-row">
          <span class="info-key">本地时间</span>
          <span class="info-value text-mono">{{ localTime || "—" }}</span>
        </div>
        <div class="info-row">
          <span class="info-key">UTC 时间</span>
          <span class="info-value text-mono">{{ utcTime || "—" }}</span>
        </div>
        <div class="info-row">
          <span class="info-key">上海日期</span>
          <span class="info-value text-mono">{{ formatDateShanghai(new Date()) }}</span>
        </div>
        <div class="info-row">
          <span class="info-key">本地时区</span>
          <span class="info-value text-mono">{{ Intl.DateTimeFormat().resolvedOptions().timeZone }}</span>
        </div>
        <div class="info-row">
          <span class="info-key">时区偏移</span>
          <span class="info-value text-mono">{{ new Date().getTimezoneOffset() }} 分钟</span>
        </div>
      </div>
    </Card>

    <!-- 2. 数据文件检查 -->
    <Card id="debug-files" padding="lg" class="debug-section">
      <div class="section-head">
        <div class="section-title">
          <FolderTree :size="18" />
          <span>数据文件检查</span>
        </div>
        <Button variant="ghost" size="sm" @click="checkDataDirs">
          <RefreshCw :size="14" />
          <span>重新检查</span>
        </Button>
      </div>

      <div v-if="!dataDir" class="empty-inline">未设置数据目录，无法检查文件。</div>

      <div v-else class="dir-list">
        <div v-for="dir in dataDirs" :key="dir.name" class="dir-item">
          <button class="dir-header" @click="toggleDir(dir.name)">
            <component
              :is="expandedDir === dir.name ? ChevronDown : ChevronRight"
              :size="14"
              class="chevron"
            />
            <FileText :size="14" class="dir-icon" />
            <span class="dir-name">{{ dir.label }}</span>
            <Badge v-if="dir.loading" variant="info" size="sm">检查中</Badge>
            <Badge v-else-if="dir.exists === true" variant="success" size="sm">
              {{ dir.entries.length }} 个文件
            </Badge>
            <Badge v-else-if="dir.exists === false" variant="danger" size="sm">缺失</Badge>
          </button>

          <div v-if="expandedDir === dir.name" class="dir-content">
            <div v-if="dir.error" class="error-text">{{ dir.error }}</div>
            <div v-else-if="dir.entries.length === 0" class="empty-inline">目录为空</div>
            <ul v-else class="file-list">
              <li
                v-for="entry in dir.entries"
                :key="entry.name"
              >
                <button
                  class="file-item"
                  :class="{ active: fileContent?.dir === dir.name && fileContent?.name === entry.name }"
                  @click="viewFile(dir.name, entry)"
                >
                  <FileText :size="13" class="file-icon" />
                  <span class="file-name">{{ entry.name }}</span>
                  <span v-if="entry.isDirectory" class="file-tag">目录</span>
                </button>
              </li>
            </ul>
          </div>
        </div>
      </div>

      <!-- 文件内容预览 -->
      <div v-if="fileContent" class="file-preview">
        <div class="preview-head">
          <span class="preview-title text-mono">{{ fileContent.dir }}/{{ fileContent.name }}</span>
          <Button variant="ghost" size="sm" icon @click="fileContent = null">
            ×
          </Button>
        </div>
        <LoadingSpinner v-if="loadingFile" :size="20" label="读取文件..." />
        <div v-else-if="fileContent.error" class="error-text">{{ fileContent.error }}</div>
        <CodeBlock v-else :code="fileContent.content" :label="`${fileContent.dir}/${fileContent.name}`" />
      </div>
    </Card>

    <!-- 3. State 解析测试 -->
    <Card id="debug-state" padding="lg" class="debug-section">
      <div class="section-head">
        <div class="section-title">
          <Boxes :size="18" />
          <span>State 解析测试</span>
        </div>
        <div class="section-actions">
          <Badge :variant="statusBadge(stateTest.status)" size="sm">
            {{ statusLabel(stateTest.status) }}
          </Badge>
          <Button variant="ghost" size="sm" @click="runStateTest">
            <RefreshCw :size="14" />
            <span>测试</span>
          </Button>
        </div>
      </div>

      <div v-if="stateTest.error" class="error-text">{{ stateTest.error }}</div>
      <LoadingSpinner v-if="stateTest.status === 'loading'" :size="20" label="调用 api.getState()..." />
      <pre v-if="stateTest.data" class="code-block">{{ formatJson(stateTest.data) }}</pre>
      <div v-if="stateTest.status === 'idle'" class="empty-inline">点击「测试」调用 api.getState()。</div>
    </Card>

    <!-- 4. Plan 解析测试 -->
    <Card id="debug-plan" padding="lg" class="debug-section">
      <div class="section-head">
        <div class="section-title">
          <Calendar :size="18" />
          <span>Plan 解析测试</span>
        </div>
        <div class="section-actions">
          <input
            v-model="planTestDate"
            type="date"
            class="form-input date-input"
          />
          <Badge :variant="statusBadge(planTest.status)" size="sm">
            {{ statusLabel(planTest.status) }}
          </Badge>
          <Button variant="ghost" size="sm" @click="runPlanTest">
            <RefreshCw :size="14" />
            <span>测试</span>
          </Button>
        </div>
      </div>

      <div v-if="planTest.error" class="error-text">{{ planTest.error }}</div>
      <div v-if="planAbnormal.length > 0" class="warn-list">
        <span class="warn-label">异常标记：</span>
        <Badge v-for="issue in planAbnormal" :key="issue" variant="warning" size="sm">
          {{ issue }}
        </Badge>
      </div>
      <LoadingSpinner v-if="planTest.status === 'loading'" :size="20" label="调用 api.getPlanByDate()..." />
      <div v-if="planTest.data" class="plan-summary">
        <div class="info-row">
          <span class="info-key">日期</span>
          <span class="info-value text-mono">{{ planTest.data.meta.date }}</span>
        </div>
        <div class="info-row">
          <span class="info-key">生成时间</span>
          <span class="info-value text-mono">{{ planTest.data.meta.generated_at || '—' }}</span>
        </div>
        <div class="info-row">
          <span class="info-key">任务数</span>
          <span class="info-value">A: {{ planTest.data.data.tasks.filter(t => t.priority === 'A').length }} · B: {{ planTest.data.data.tasks.filter(t => t.priority === 'B').length }} · 合计: {{ planTest.data.data.total_tasks }}</span>
        </div>
        <div class="info-row">
          <span class="info-key">完成状态</span>
          <span class="info-value">
            已完成 {{ planTest.data.data.tasks.filter((t) => t.status === 'done').length }}
            / {{ planTest.data.data.tasks.length }}
          </span>
        </div>
        <div class="info-row">
          <span class="info-key">目标</span>
          <span class="info-value">{{ planTest.data.data.target || '—' }}</span>
        </div>
      </div>
      <pre v-if="planTest.data" class="code-block">{{ formatJson(planTest.data) }}</pre>
      <div v-if="planTest.status === 'idle'" class="empty-inline">点击「测试」调用 api.getPlanByDate()。</div>
    </Card>

    <!-- 5. Review 解析测试 -->
    <Card id="debug-review" padding="lg" class="debug-section">
      <div class="section-head">
        <div class="section-title">
          <FileCheck :size="18" />
          <span>Review 解析测试</span>
        </div>
        <div class="section-actions">
          <input
            v-model="reviewTestDate"
            type="date"
            class="form-input date-input"
          />
          <Badge :variant="statusBadge(reviewTest.status)" size="sm">
            {{ statusLabel(reviewTest.status) }}
          </Badge>
          <Button variant="ghost" size="sm" @click="runReviewTest">
            <RefreshCw :size="14" />
            <span>测试</span>
          </Button>
        </div>
      </div>

      <div v-if="reviewTest.error" class="error-text">{{ reviewTest.error }}</div>
      <div v-if="reviewAbnormal.length > 0" class="warn-list">
        <span class="warn-label">异常标记：</span>
        <Badge v-for="issue in reviewAbnormal" :key="issue" variant="warning" size="sm">
          {{ issue }}
        </Badge>
      </div>
      <LoadingSpinner v-if="reviewTest.status === 'loading'" :size="20" label="调用 api.getReview()..." />
      <div v-if="reviewTest.data" class="plan-summary">
        <div class="info-row">
          <span class="info-key">日期</span>
          <span class="info-value text-mono">{{ reviewTest.data.meta.date }}</span>
        </div>
        <div class="info-row">
          <span class="info-key">完成率</span>
          <span class="info-value">{{ reviewTest.data.data.completion.completion_rate }}% (A: {{ reviewTest.data.data.completion.priority_a_done }}/{{ reviewTest.data.data.completion.priority_a_total }} · B: {{ reviewTest.data.data.completion.priority_b_done }}/{{ reviewTest.data.data.completion.priority_b_total }})</span>
        </div>
        <div class="info-row">
          <span class="info-key">总时长</span>
          <span class="info-value">{{ reviewTest.data.data.total_hours }}h</span>
        </div>
        <div class="info-row">
          <span class="info-key">精力评分</span>
          <span class="info-value">{{ reviewTest.data.data.energy_level }}/5</span>
        </div>
      </div>
      <pre v-if="reviewTest.data" class="code-block">{{ formatJson(reviewTest.data) }}</pre>
      <div v-if="reviewTest.status === 'idle'" class="empty-inline">点击「测试」调用 api.getReview()。</div>
    </Card>

    <!-- 6. Dashboard 数据测试 -->
    <Card id="debug-dashboard" padding="lg" class="debug-section">
      <div class="section-head">
        <div class="section-title">
          <LayoutDashboard :size="18" />
          <span>Dashboard 数据测试</span>
        </div>
        <div class="section-actions">
          <Badge :variant="statusBadge(dashboardTest.status)" size="sm">
            {{ statusLabel(dashboardTest.status) }}
          </Badge>
          <Button variant="ghost" size="sm" @click="runDashboardTest">
            <RefreshCw :size="14" />
            <span>测试</span>
          </Button>
        </div>
      </div>

      <div v-if="dashboardTest.error" class="error-text">{{ dashboardTest.error }}</div>
      <div v-if="dashboardAbnormal.length > 0" class="warn-list">
        <span class="warn-label">异常标记：</span>
        <Badge v-for="issue in dashboardAbnormal" :key="issue" variant="warning" size="sm">
          {{ issue }}
        </Badge>
      </div>
      <LoadingSpinner v-if="dashboardTest.status === 'loading'" :size="20" label="调用 api.getDashboardSummary()..." />
      <pre v-if="dashboardTest.data" class="code-block">{{ formatJson(dashboardTest.data) }}</pre>
      <div v-if="dashboardTest.status === 'idle'" class="empty-inline">点击「测试」调用 api.getDashboardSummary()。</div>
    </Card>

    <!-- 7. AI Provider 测试 -->
    <Card id="debug-providers" padding="lg" class="debug-section">
      <div class="section-head">
        <div class="section-title">
          <Bot :size="18" />
          <span>AI Provider 测试</span>
        </div>
        <Button
          v-if="providerTests.length > 0"
          variant="ghost"
          size="sm"
          @click="testAllProviders"
        >
          <RefreshCw :size="14" />
          <span>全部测试</span>
        </Button>
      </div>

      <div v-if="providerTests.length === 0" class="empty-inline">
        尚未配置 AI Provider。
      </div>

      <div v-else class="provider-list">
        <div v-for="(item, idx) in providerTests" :key="item.provider.id" class="provider-row">
          <div class="provider-info">
            <span class="provider-name">{{ item.provider.name }}</span>
            <span class="provider-sub text-mono">{{ item.provider.type }} · {{ item.provider.model }}</span>
          </div>
          <div class="provider-actions">
            <Badge :variant="statusBadge(item.status)" size="sm">
              {{ statusLabel(item.status) }}
            </Badge>
            <Button variant="secondary" size="sm" :loading="item.status === 'loading'" @click="testProvider(idx)">
              <Zap :size="14" />
              <span>测试</span>
            </Button>
          </div>
          <div v-if="item.message" class="provider-message" :class="{ error: item.status === 'error' }">
            {{ item.message }}
          </div>
        </div>
      </div>
    </Card>

    <!-- 8. AI 调用记录 -->
    <Card id="debug-ai-calls" padding="lg" class="debug-section">
      <div class="section-head">
        <div class="section-title">
          <Radio :size="18" />
          <span>AI 调用记录</span>
        </div>
        <Button
          variant="ghost"
          size="sm"
          :disabled="aiDebugStore.records.length === 0"
          @click="aiDebugStore.clearAll()"
        >
          <Trash2 :size="14" />
          <span>清空</span>
        </Button>
      </div>

      <p class="section-desc">
        实时记录所有 AI 调用：请求参数、响应数据、耗时与错误。最多保留 50 条；
        后端原始响应（HTTP body / SSE 行）可在终端日志中查看，前缀为 <code class="text-mono">[AI-DEBUG]</code>。
      </p>

      <div class="info-row">
        <span class="info-key">记录总数</span>
        <span class="info-value text-mono">{{ aiDebugStore.records.length }}</span>
      </div>

      <div v-if="aiDebugStore.records.length === 0" class="empty-inline">
        暂无 AI 调用记录。生成计划、生成复盘或在助手页发送对话后会显示在此。
      </div>

      <div v-else class="ai-call-list">
        <div
          v-for="rec in aiDebugStore.records"
          :key="rec.id"
          class="ai-call-item"
          :class="{ expanded: expandedAiCallId === rec.id }"
        >
          <button class="ai-call-header" @click="toggleAiCall(rec.id)">
            <ChevronRight :size="14" class="ai-call-chevron" :class="{ open: expandedAiCallId === rec.id }" />
            <span class="ai-call-time text-mono">{{ formatTimestamp(rec.timestamp) }}</span>
            <Badge :variant="aiCallStatusBadge(rec.status)" size="sm">
              {{ aiCallStatusLabel(rec.status) }}
            </Badge>
            <span class="ai-call-label">{{ rec.label }}</span>
            <span class="ai-call-cmd text-mono">{{ rec.command }}</span>
            <span class="ai-call-duration text-mono">{{ formatDuration(rec.durationMs) }}</span>
          </button>

          <div v-if="expandedAiCallId === rec.id" class="ai-call-detail">
            <div class="ai-call-block">
              <div class="ai-call-block-head">
                <Send :size="13" />
                <span>请求参数</span>
              </div>
              <pre class="code-block">{{ formatJson(rec.request) }}</pre>
            </div>

            <div v-if="rec.status === 'success'" class="ai-call-block">
              <div class="ai-call-block-head">
                <CheckCircle2 :size="13" />
                <span>响应数据</span>
              </div>
              <pre class="code-block">{{ formatJson(rec.response) }}</pre>
            </div>

            <div v-if="rec.status === 'error'" class="ai-call-block">
              <div class="ai-call-block-head error-head">
                <AlertTriangle :size="13" />
                <span>错误信息</span>
              </div>
              <pre class="code-block error-block">{{ rec.error }}</pre>
            </div>
          </div>
        </div>
      </div>
    </Card>

    <!-- 10. AI 用量日志（持久化，含费用估算） -->
    <Card id="debug-ai-usage" padding="lg" class="debug-section">
      <div class="section-head">
        <div class="section-title">
          <Coins :size="18" />
          <span>AI 用量日志</span>
        </div>
        <div class="section-actions">
          <Button
            variant="ghost"
            size="sm"
            :loading="aiUsageLoading"
            @click="loadAiUsageLog"
          >
            <RefreshCw :size="14" />
            <span>刷新</span>
          </Button>
          <Button
            variant="ghost"
            size="sm"
            :disabled="aiUsageLog.length === 0"
            @click="clearAiUsageLog"
          >
            <Trash2 :size="14" />
            <span>清空</span>
          </Button>
        </div>
      </div>

      <p class="section-desc">
        根据各厂商官方定价估算费用，仅供参考。{{ pricingNote }}
      </p>

      <!-- 时间筛选 -->
      <div class="usage-filter">
        <button
          v-for="opt in [
            { value: 'all', label: '全部' },
            { value: 'today', label: '近 24h' },
            { value: '7d', label: '近 7 天' },
            { value: '30d', label: '近 30 天' },
          ]"
          :key="opt.value"
          class="usage-filter-btn"
          :class="{ active: usageTimeFilter === opt.value }"
          @click="usageTimeFilter = opt.value as typeof usageTimeFilter"
        >
          {{ opt.label }}
        </button>
      </div>

      <div v-if="aiUsageError" class="error-text">{{ aiUsageError }}</div>
      <LoadingSpinner v-if="aiUsageLoading" :size="20" label="加载 AI 用量日志..." />

      <!-- 汇总卡片 -->
      <div v-if="!aiUsageLoading && usageSummary.totalCalls > 0" class="usage-summary">
        <div class="usage-summary-grid">
          <div class="usage-stat-card">
            <span class="usage-stat-label">总调用次数</span>
            <span class="usage-stat-value text-mono">{{ usageSummary.totalCalls }}</span>
            <span class="usage-stat-sub">
              成功 {{ usageSummary.successCalls }} · 失败 {{ usageSummary.errorCalls }}
            </span>
          </div>
          <div class="usage-stat-card">
            <span class="usage-stat-label">输入 Token</span>
            <span class="usage-stat-value text-mono">{{ formatTokens(usageSummary.totalInput) }}</span>
            <span class="usage-stat-sub">{{ usageSummary.totalInput.toLocaleString() }} tokens</span>
          </div>
          <div class="usage-stat-card">
            <span class="usage-stat-label">输出 Token</span>
            <span class="usage-stat-value text-mono">{{ formatTokens(usageSummary.totalOutput) }}</span>
            <span class="usage-stat-sub">{{ usageSummary.totalOutput.toLocaleString() }} tokens</span>
          </div>
          <div class="usage-stat-card usage-stat-cost">
            <span class="usage-stat-label">估算总费用</span>
            <span class="usage-stat-value text-mono">{{ formatCost(usageSummary.totalCost) }}</span>
            <span class="usage-stat-sub">人民币（估算）</span>
          </div>
          <div class="usage-stat-card">
            <span class="usage-stat-label">总耗时</span>
            <span class="usage-stat-value text-mono">{{ formatUsageDuration(usageSummary.totalDurationMs) }}</span>
            <span class="usage-stat-sub">平均 {{ formatUsageDuration(usageSummary.avgDurationMs) }}/次</span>
          </div>
        </div>

        <!-- 按模型分组 -->
        <div v-if="usageSummary.byModel.length > 0" class="usage-breakdown">
          <div class="usage-breakdown-title">按模型分组</div>
          <div class="usage-breakdown-table">
            <div class="usage-row usage-row-head">
              <span class="usage-col-model">模型</span>
              <span class="usage-col-num">调用次数</span>
              <span class="usage-col-num">输入</span>
              <span class="usage-col-num">输出</span>
              <span class="usage-col-cost">费用</span>
            </div>
            <div
              v-for="row in usageSummary.byModel"
              :key="row.model"
              class="usage-row"
            >
              <span class="usage-col-model text-mono">{{ row.model }}</span>
              <span class="usage-col-num text-mono">{{ row.calls }}</span>
              <span class="usage-col-num text-mono">{{ formatTokens(row.input) }}</span>
              <span class="usage-col-num text-mono">{{ formatTokens(row.output) }}</span>
              <span class="usage-col-cost text-mono">{{ formatCost(row.cost) }}</span>
            </div>
          </div>
        </div>

        <!-- 按 Agent 分组 -->
        <div v-if="usageSummary.byAgent.length > 0" class="usage-breakdown">
          <div class="usage-breakdown-title">按 Agent 类型分组</div>
          <div class="usage-breakdown-table">
            <div class="usage-row usage-row-head">
              <span class="usage-col-model">Agent</span>
              <span class="usage-col-num">调用次数</span>
              <span class="usage-col-num">输入</span>
              <span class="usage-col-num">输出</span>
              <span class="usage-col-cost">费用</span>
            </div>
            <div
              v-for="row in usageSummary.byAgent"
              :key="row.agent"
              class="usage-row"
            >
              <span class="usage-col-model">{{ agentLabel(row.agent) }}</span>
              <span class="usage-col-num text-mono">{{ row.calls }}</span>
              <span class="usage-col-num text-mono">{{ formatTokens(row.input) }}</span>
              <span class="usage-col-num text-mono">{{ formatTokens(row.output) }}</span>
              <span class="usage-col-cost text-mono">{{ formatCost(row.cost) }}</span>
            </div>
          </div>
        </div>
      </div>

      <!-- 用量记录列表 -->
      <div v-if="!aiUsageLoading && filteredUsageLog.length > 0" class="usage-list">
        <div class="usage-list-head">调用明细（{{ filteredUsageLog.length }} 条，最新在前）</div>
        <div
          v-for="(entry, idx) in filteredUsageLog"
          :key="idx"
          class="usage-item"
          :class="{ expanded: expandedUsageIdx === idx }"
        >
          <button class="usage-item-header" @click="toggleUsageEntry(idx)">
            <ChevronRight :size="14" class="ai-call-chevron" :class="{ open: expandedUsageIdx === idx }" />
            <span class="usage-item-time text-mono">{{ formatUsageTimestamp(entry.timestamp) }}</span>
            <Badge :variant="usageStatusBadge(entry.status)" size="sm">
              {{ usageStatusLabel(entry.status) }}
            </Badge>
            <Badge variant="default" size="sm">{{ agentLabel(entry.agent) }}</Badge>
            <span class="usage-item-model text-mono">{{ entry.model || "(unknown)" }}</span>
            <span class="usage-item-tokens text-mono">
              ↑{{ formatTokens(entry.prompt_tokens) }} · ↓{{ formatTokens(entry.completion_tokens) }}
            </span>
            <span class="usage-item-duration text-mono">{{ formatUsageDuration(entry.duration_ms) }}</span>
            <span class="usage-item-cost text-mono">
              {{ formatCost(usageCostMap.get(usageEntryKey(entry))?.costCny ?? 0) }}
            </span>
          </button>

          <div v-if="expandedUsageIdx === idx" class="usage-item-detail">
            <div class="info-row">
              <span class="info-key">时间</span>
              <span class="info-value text-mono">{{ formatUsageTimestamp(entry.timestamp) }}</span>
            </div>
            <div class="info-row">
              <span class="info-key">模型</span>
              <span class="info-value text-mono">{{ entry.model || "—" }}</span>
            </div>
            <div class="info-row">
              <span class="info-key">Agent</span>
              <span class="info-value">{{ agentLabel(entry.agent) }}（{{ entry.agent }}）</span>
            </div>
            <div class="info-row">
              <span class="info-key">输入 Token</span>
              <span class="info-value text-mono">{{ entry.prompt_tokens.toLocaleString() }}</span>
            </div>
            <div class="info-row">
              <span class="info-key">输出 Token</span>
              <span class="info-value text-mono">{{ entry.completion_tokens.toLocaleString() }}</span>
            </div>
            <div class="info-row">
              <span class="info-key">总 Token</span>
              <span class="info-value text-mono">{{ entry.total_tokens.toLocaleString() }}</span>
            </div>
            <div class="info-row">
              <span class="info-key">耗时</span>
              <span class="info-value text-mono">{{ formatUsageDuration(entry.duration_ms) }}</span>
            </div>
            <div class="info-row">
              <span class="info-key">状态</span>
              <span class="info-value">
                <Badge :variant="usageStatusBadge(entry.status)" size="sm">
                  {{ usageStatusLabel(entry.status) }}
                </Badge>
              </span>
            </div>
            <div class="info-row">
              <span class="info-key">费用估算</span>
              <span class="info-value">
                <span class="usage-cost-value text-mono">
                  {{ formatCost(usageCostMap.get(usageEntryKey(entry))?.costCny ?? 0) }}
                </span>
                <span class="usage-cost-note">
                  {{ usageCostMap.get(usageEntryKey(entry))?.note ?? "—" }}
                </span>
              </span>
            </div>
            <div v-if="entry.error" class="info-row">
              <span class="info-key">错误信息</span>
              <span class="info-value error-inline">{{ entry.error }}</span>
            </div>
          </div>
        </div>
      </div>

      <div v-if="!aiUsageLoading && filteredUsageLog.length === 0 && !aiUsageError" class="empty-inline">
        {{ aiUsageLog.length === 0 ? "暂无 AI 用量记录。生成计划、生成复盘或在助手页发送对话后会显示在此。" : "当前筛选条件下无记录。" }}
      </div>
    </Card>

    <!-- 11. Settings 查看 -->
    <Card id="debug-settings" padding="lg" class="debug-section">
      <div class="section-head">
        <div class="section-title">
          <Settings2 :size="18" />
          <span>Settings 查看</span>
        </div>
        <Button variant="ghost" size="sm" @click="loadSettingsView">
          <RefreshCw :size="14" />
          <span>刷新</span>
        </Button>
      </div>

      <div class="info-row">
        <span class="info-key">配置文件路径</span>
        <span class="info-value text-mono break-all">{{ configPath || "—" }}</span>
      </div>
      <div v-if="settingsView.error" class="error-text">{{ settingsView.error }}</div>
      <LoadingSpinner v-if="settingsView.status === 'loading'" :size="20" label="加载设置..." />
      <pre v-if="settingsView.data" class="code-block">{{ formatJson(settingsView.data) }}</pre>
    </Card>

    <!-- 12. 日志查看 -->
    <Card id="debug-logs" padding="lg" class="debug-section">
      <div class="section-head">
        <div class="section-title">
          <ScrollText :size="18" />
          <span>日志查看</span>
        </div>
        <Button variant="ghost" size="sm" @click="clearLogs">
          <Trash2 :size="14" />
          <span>清除</span>
        </Button>
      </div>

      <div class="log-toolbar">
        <Clock :size="13" class="log-toolbar-icon" />
        <span class="log-count">共 {{ logs.length }} 条</span>
      </div>

      <div v-if="logs.length === 0" class="empty-inline">暂无日志。</div>
      <div v-else class="log-list">
        <div
          v-for="(log, idx) in logs"
          :key="idx"
          class="log-item"
          :class="log.level"
        >
          <span class="log-time text-mono">{{ log.time }}</span>
          <span class="log-level">{{ log.level.toUpperCase() }}</span>
          <span class="log-text">{{ log.args }}</span>
        </div>
      </div>
    </Card>
      </div>
    </div>
  </div>
</template>

<style scoped>
.debug-view {
  padding: var(--space-6) var(--space-8) var(--space-12);
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.debug-container {
  width: 100%;
  max-width: 1080px;
  margin: 0 auto;
  display: flex;
  flex-direction: row;
  align-items: flex-start;
  gap: var(--space-6);
}

/* ── 左侧快速导航 ── */
.debug-nav-side {
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

.debug-nav-side::-webkit-scrollbar {
  display: none;
}

.debug-nav-side .nav-item {
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

.debug-nav-side .nav-item:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.debug-nav-side .nav-item.active {
  background: var(--accent-subtle);
  color: var(--accent);
}

.debug-content {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.debug-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  flex-wrap: wrap;
}

.debug-desc {
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.debug-section {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

/* ── 区块标题 ── */
.section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
}

.section-title {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.section-actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

/* ── 信息网格 ── */
.info-grid {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.info-row {
  display: flex;
  align-items: baseline;
  gap: var(--space-3);
}

.info-key {
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  color: var(--text-tertiary);
  min-width: 96px;
  flex-shrink: 0;
}

.info-value {
  font-size: var(--text-sm);
  color: var(--text-primary);
}

.text-mono {
  font-family: var(--font-mono);
}

.break-all {
  word-break: break-all;
}

/* ── 目录检查 ── */
.dir-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.dir-item {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.dir-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  width: 100%;
  padding: var(--space-2) var(--space-3);
  border: none;
  background: var(--bg-tertiary);
  border-radius: var(--radius-sm);
  font-family: inherit;
  font-size: var(--text-sm);
  color: var(--text-primary);
  cursor: pointer;
  transition: background var(--transition-fast);
  text-align: left;
}

.dir-header:hover {
  background: var(--sidebar-item-hover);
}

.chevron {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.dir-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.dir-name {
  flex: 1;
}

.dir-content {
  padding-left: var(--space-6);
}

.file-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.file-item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  width: 100%;
  padding: var(--space-1) var(--space-2);
  border: none;
  background: transparent;
  font-family: inherit;
  font-size: var(--text-xs);
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: var(--radius-xs);
  transition: all var(--transition-fast);
  text-align: left;
}

.file-item:hover {
  background: var(--sidebar-item-hover);
  color: var(--text-primary);
}

.file-item.active {
  background: var(--accent-subtle);
  color: var(--accent);
}

.file-icon {
  flex-shrink: 0;
  opacity: 0.7;
}

.file-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-tag {
  font-size: 10px;
  color: var(--text-quaternary);
  flex-shrink: 0;
}

/* ── 文件预览 ── */
.file-preview {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
}

.preview-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
}

.preview-title {
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

/* ── 代码块 ── */
.code-block {
  margin: 0;
  padding: var(--space-4);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  line-height: var(--leading-relaxed);
  color: var(--text-primary);
  overflow-x: auto;
  white-space: pre;
  max-height: 360px;
  overflow-y: auto;
}

/* ── 状态文本 ── */
.error-text {
  font-size: var(--text-sm);
  color: var(--color-danger);
  padding: var(--space-2) var(--space-3);
  background: var(--color-danger-subtle);
  border-radius: var(--radius-sm);
}

.warn-list {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.plan-summary {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
  margin-bottom: var(--space-3);
}

.date-input {
  width: 140px;
  padding: var(--space-1) var(--space-2);
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  color: var(--text-primary);
  font-size: var(--text-xs);
  font-family: inherit;
}

.warn-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.empty-inline {
  font-size: var(--text-sm);
  color: var(--text-tertiary);
  padding: var(--space-2) 0;
}

/* ── Provider 测试 ── */
.provider-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.provider-row {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
  flex-wrap: wrap;
}

.provider-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.provider-name {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--text-primary);
}

.provider-sub {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.provider-actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-shrink: 0;
}

.provider-message {
  width: 100%;
  font-size: var(--text-xs);
  color: var(--color-success);
  padding: var(--space-1) var(--space-2);
  background: var(--color-success-subtle);
  border-radius: var(--radius-xs);
}

.provider-message.error {
  color: var(--color-danger);
  background: var(--color-danger-subtle);
}

/* ── 日志 ── */
.log-toolbar {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.log-toolbar-icon {
  color: var(--text-tertiary);
}

.log-count {
  font-weight: var(--font-medium);
}

.log-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  max-height: 320px;
  overflow-y: auto;
  padding: var(--space-2);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
}

.log-item {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  font-size: var(--text-xs);
  line-height: var(--leading-relaxed);
  font-family: var(--font-mono);
}

.log-time {
  color: var(--text-quaternary);
  flex-shrink: 0;
}

.log-level {
  flex-shrink: 0;
  font-weight: var(--font-semibold);
  min-width: 48px;
}

.log-item.log .log-level { color: var(--text-tertiary); }
.log-item.info .log-level { color: var(--color-info); }
.log-item.warn .log-level { color: var(--color-warning); }
.log-item.error .log-level { color: var(--color-danger); }

.log-text {
  color: var(--text-secondary);
  word-break: break-all;
  flex: 1;
}

.log-item.error .log-text { color: var(--color-danger); }
.log-item.warn .log-text { color: var(--text-primary); }

/* ── AI 调用记录 ── */
.ai-call-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  margin-top: var(--space-3);
}

.ai-call-item {
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--bg-secondary);
  transition: border-color var(--transition-fast);
}

.ai-call-item.expanded {
  border-color: var(--accent);
}

.ai-call-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  width: 100%;
  padding: var(--space-2) var(--space-3);
  border: none;
  background: transparent;
  font-family: inherit;
  font-size: var(--text-xs);
  color: var(--text-secondary);
  cursor: pointer;
  text-align: left;
  transition: background var(--transition-fast);
}

.ai-call-header:hover {
  background: var(--sidebar-item-hover);
}

.ai-call-chevron {
  flex-shrink: 0;
  color: var(--text-quaternary);
  transition: transform var(--transition-fast);
}

.ai-call-chevron.open {
  transform: rotate(90deg);
}

.ai-call-time {
  flex-shrink: 0;
  color: var(--text-quaternary);
  min-width: 64px;
}

.ai-call-label {
  flex: 1;
  color: var(--text-primary);
  font-weight: var(--font-medium);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ai-call-cmd {
  flex-shrink: 0;
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  padding: 1px var(--space-1);
  background: var(--bg-tertiary);
  border-radius: var(--radius-xs);
}

.ai-call-duration {
  flex-shrink: 0;
  color: var(--text-quaternary);
  min-width: 56px;
  text-align: right;
}

.ai-call-detail {
  padding: var(--space-2) var(--space-3) var(--space-3);
  border-top: 1px solid var(--border-color);
  background: var(--bg-primary);
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.ai-call-block {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.ai-call-block-head {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--text-tertiary);
}

.ai-call-block-head.error-head {
  color: var(--color-danger);
}

.ai-call-detail .code-block {
  margin: 0;
  max-height: 320px;
  overflow: auto;
  font-size: var(--text-xs);
}

.ai-call-detail .error-block {
  color: var(--color-danger);
  white-space: pre-wrap;
}

/* ── AI 用量日志 ── */
.section-desc {
  font-size: var(--text-sm);
  color: var(--text-tertiary);
  line-height: var(--leading-relaxed);
  margin: 0;
}

.usage-filter {
  display: flex;
  gap: var(--space-1);
  flex-wrap: wrap;
}

.usage-filter-btn {
  padding: var(--space-1) var(--space-3);
  border: 1px solid var(--border-color);
  background: var(--bg-elevated);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-family: inherit;
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: all var(--transition-fast);
}

.usage-filter-btn:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.usage-filter-btn.active {
  background: var(--accent-subtle);
  border-color: var(--accent);
  color: var(--accent);
  font-weight: var(--font-medium);
}

/* 汇总卡片 */
.usage-summary {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.usage-summary-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
  gap: var(--space-3);
}

.usage-stat-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  padding: var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
  border: 1px solid var(--border-color);
}

.usage-stat-card.usage-stat-cost {
  background: var(--accent-subtle);
  border-color: var(--accent);
}

.usage-stat-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-weight: var(--font-medium);
}

.usage-stat-value {
  font-size: var(--text-xl);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  line-height: 1.2;
}

.usage-stat-card.usage-stat-cost .usage-stat-value {
  color: var(--accent);
}

.usage-stat-sub {
  font-size: var(--text-xs);
  color: var(--text-quaternary);
}

/* 分组明细 */
.usage-breakdown {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.usage-breakdown-title {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-secondary);
}

.usage-breakdown-table {
  display: flex;
  flex-direction: column;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  overflow: hidden;
}

.usage-row {
  display: grid;
  grid-template-columns: 2fr 1fr 1fr 1fr 1fr;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  font-size: var(--text-xs);
  align-items: center;
  border-bottom: 1px solid var(--border-color);
}

.usage-row:last-child {
  border-bottom: none;
}

.usage-row-head {
  background: var(--bg-tertiary);
  font-weight: var(--font-semibold);
  color: var(--text-tertiary);
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.usage-row:not(.usage-row-head):hover {
  background: var(--sidebar-item-hover);
}

.usage-col-model {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-primary);
}

.usage-col-num {
  text-align: right;
  color: var(--text-secondary);
}

.usage-col-cost {
  text-align: right;
  color: var(--accent);
  font-weight: var(--font-medium);
}

/* 用量记录列表 */
.usage-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  margin-top: var(--space-3);
}

.usage-list-head {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-secondary);
}

.usage-item {
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--bg-secondary);
  transition: border-color var(--transition-fast);
}

.usage-item.expanded {
  border-color: var(--accent);
}

.usage-item-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  width: 100%;
  padding: var(--space-2) var(--space-3);
  border: none;
  background: transparent;
  font-family: inherit;
  font-size: var(--text-xs);
  color: var(--text-secondary);
  cursor: pointer;
  text-align: left;
  transition: background var(--transition-fast);
  flex-wrap: wrap;
}

.usage-item-header:hover {
  background: var(--sidebar-item-hover);
}

.usage-item-time {
  flex-shrink: 0;
  color: var(--text-quaternary);
  min-width: 140px;
}

.usage-item-model {
  flex: 1;
  min-width: 100px;
  color: var(--text-primary);
  font-weight: var(--font-medium);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.usage-item-tokens {
  flex-shrink: 0;
  color: var(--text-tertiary);
  font-size: 11px;
}

.usage-item-duration {
  flex-shrink: 0;
  color: var(--text-quaternary);
  min-width: 56px;
  text-align: right;
}

.usage-item-cost {
  flex-shrink: 0;
  color: var(--accent);
  font-weight: var(--font-semibold);
  min-width: 72px;
  text-align: right;
}

.usage-item-detail {
  padding: var(--space-3);
  border-top: 1px solid var(--border-color);
  background: var(--bg-primary);
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.usage-item-detail .info-row {
  align-items: center;
}

.usage-cost-value {
  display: inline-block;
  font-weight: var(--font-semibold);
  color: var(--accent);
  margin-right: var(--space-2);
}

.usage-cost-note {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.error-inline {
  color: var(--color-danger);
  font-size: var(--text-xs);
  word-break: break-all;
}

/* ── 响应式 ── */
@media (max-width: 720px) {
  .info-key {
    min-width: 80px;
  }
  .usage-row {
    grid-template-columns: 1.5fr 1fr 1fr;
  }
  .usage-row .usage-col-num:nth-child(3),
  .usage-row .usage-col-num:nth-child(4) {
    display: none;
  }
  .usage-item-time {
    min-width: 120px;
  }
}
</style>
