<script setup lang="ts">
/**
 * 调试页 — 主容器
 *
 * 由原 DebugView.vue 拆分而来：只负责页面布局、统一刷新编排与左侧锚点导航，
 * 各诊断区块的具体实现已迁移到独立的面板组件 / composables。
 * 共享样式见 debug-base.css。
 */
import { ref, onMounted, onUnmounted } from "vue";
import type { Component } from "vue";
import Button from "@/components/ui/Button.vue";
import {
  RefreshCw,
  Cpu,
  FolderTree,
  Boxes,
  LayoutDashboard,
  Bot,
  Settings2,
  ScrollText,
  Calendar,
  FileCheck,
  Radio,
  Coins,
  AlertCircle,
} from "lucide-vue-next";
import { useDebugRefresh } from "./composables/useDebugRefresh";
import SystemInfoPanel from "./components/SystemInfoPanel.vue";
import DataDirectoryPanel from "./components/DataDirectoryPanel.vue";
import StateProbePanel from "./components/StateProbePanel.vue";
import PlanProbePanel from "./components/PlanProbePanel.vue";
import ReviewProbePanel from "./components/ReviewProbePanel.vue";
import DashboardProbePanel from "./components/DashboardProbePanel.vue";
import ProviderDiagnosticsPanel from "./components/ProviderDiagnosticsPanel.vue";
import AiCallsPanel from "./components/AiCallsPanel.vue";
import AiUsagePanel from "./components/AiUsagePanel.vue";
import SettingsProbePanel from "./components/SettingsProbePanel.vue";
import AppLogPanel from "./components/AppLogPanel.vue";
import "./debug-base.css";

// ── 快速导航 ──
interface DebugSection {
  id: string;
  label: string;
  icon: Component;
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

// ── 数据目录（Settings 面板上报，供系统信息 / 数据目录检查使用） ──
const dataDir = ref("");

function onDataDirChange(dir: string) {
  dataDir.value = dir;
}

// ── 统一刷新 ──
const { refreshing, refreshErrors, refreshAll, pushRefreshError } = useDebugRefresh();

// 各面板引用（通过 defineExpose 暴露的 refresh 方法编排统一刷新）
const settingsPanel = ref<InstanceType<typeof SettingsProbePanel> | null>(null);
const providerPanel = ref<InstanceType<typeof ProviderDiagnosticsPanel> | null>(null);
const statePanel = ref<InstanceType<typeof StateProbePanel> | null>(null);
const dashboardPanel = ref<InstanceType<typeof DashboardProbePanel> | null>(null);
const planPanel = ref<InstanceType<typeof PlanProbePanel> | null>(null);
const reviewPanel = ref<InstanceType<typeof ReviewProbePanel> | null>(null);
const aiUsagePanel = ref<InstanceType<typeof AiUsagePanel> | null>(null);
const dataDirPanel = ref<InstanceType<typeof DataDirectoryPanel> | null>(null);

/** 刷新全部：并发执行各面板刷新，任一失败不阻塞其余调用（H42） */
async function onRefreshAll() {
  await refreshAll([
    () => settingsPanel.value?.refresh() ?? Promise.resolve(),
    () => providerPanel.value?.refresh() ?? Promise.resolve(),
    () => statePanel.value?.refresh() ?? Promise.resolve(),
    () => dashboardPanel.value?.refresh() ?? Promise.resolve(),
    () => planPanel.value?.refresh() ?? Promise.resolve(),
    () => reviewPanel.value?.refresh() ?? Promise.resolve(),
    () => aiUsagePanel.value?.refresh() ?? Promise.resolve(),
  ]);
  // 数据目录检查依赖 Settings 面板上报的 dataDir，需在其后执行
  await dataDirPanel.value?.refresh().catch((e) => {
    pushRefreshError(`数据目录检查跳过：${String(e)}`);
  });
}

onMounted(async () => {
  await onRefreshAll();
  initSectionObserver();
});

onUnmounted(() => {
  // H34：卸载时断开 IntersectionObserver，避免跨路由内存泄漏
  sectionObserver?.disconnect();
  sectionObserver = null;
});
</script>

<template>
  <div class="debug-view">
    <!-- 顶部操作栏 -->
    <header class="debug-header">
      <p class="debug-desc">用于排查问题，查看系统信息、数据文件、API 响应与运行日志。</p>
      <Button variant="secondary" size="sm" :loading="refreshing" @click="onRefreshAll">
        <RefreshCw :size="14" />
        <span>刷新全部</span>
      </Button>
    </header>

    <!-- H42：刷新失败的逐项错误提示（部分模块失败不阻塞其他模块） -->
    <div v-if="refreshErrors.length > 0" class="refresh-errors">
      <div class="refresh-errors-title">
        <AlertCircle :size="14" />
        <span>部分模块刷新失败（{{ refreshErrors.length }} 项）</span>
      </div>
      <ul class="refresh-errors-list">
        <li v-for="(err, idx) in refreshErrors" :key="idx">{{ err }}</li>
      </ul>
    </div>

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
        <SystemInfoPanel :data-dir="dataDir" />
        <DataDirectoryPanel ref="dataDirPanel" :data-dir="dataDir" />
        <StateProbePanel ref="statePanel" />
        <PlanProbePanel ref="planPanel" />
        <ReviewProbePanel ref="reviewPanel" />
        <DashboardProbePanel ref="dashboardPanel" />
        <ProviderDiagnosticsPanel ref="providerPanel" />
        <AiCallsPanel />
        <AiUsagePanel ref="aiUsagePanel" />
        <SettingsProbePanel ref="settingsPanel" @data-dir-change="onDataDirChange" />
        <AppLogPanel />
      </div>
    </div>
  </div>
</template>
