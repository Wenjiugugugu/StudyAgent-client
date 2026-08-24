<script setup lang="ts">
/**
 * 调试页 — 系统信息面板
 *
 * 展示应用版本、运行环境、数据目录与本地/UTC 时钟。
 */
import { ref, computed, onMounted, onUnmounted } from "vue";
import Card from "@/components/ui/Card.vue";
import { Cpu } from "lucide-vue-next";
import { isTauri } from "../api";
import { formatDateShanghai } from "@/utils/date";
import { useAppVersion } from "@/version";

const props = defineProps<{ dataDir: string }>();

const TAURI_VERSION = "2.x";
const { version } = useAppVersion();
const sysInfo = computed(() => ({
  appVersion: version.value,
  tauriVersion: isTauri() ? TAURI_VERSION : "未运行（浏览器模式）",
  environment: isTauri() ? "Tauri 桌面应用" : "浏览器开发模式",
  dataDirectory: props.dataDir || "未设置",
}));

const localTime = ref("");
const utcTime = ref("");
let timeTimer: number | undefined;

function updateClock() {
  const now = new Date();
  localTime.value = now.toLocaleString("zh-CN", { hour12: false });
  utcTime.value = now.toISOString();
}

onMounted(() => {
  updateClock();
  timeTimer = window.setInterval(updateClock, 1000);
});

onUnmounted(() => {
  if (timeTimer) window.clearInterval(timeTimer);
});
</script>

<template>
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
</template>
