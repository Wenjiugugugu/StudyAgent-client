<script setup lang="ts">
/**
 * 调试页 — 应用日志面板
 *
 * 展示 logs/ai-debug.log 的内容（最新在最上面、向下越旧），挂载时自动加载。
 */
import { ref, computed, onMounted } from "vue";
import Card from "@/components/ui/Card.vue";
import Button from "@/components/ui/Button.vue";
import { ScrollText, RefreshCw, Trash2 } from "lucide-vue-next";
import { debugApi } from "../api";

const appLog = ref("");
const appLogLoading = ref(false);
const appLogError = ref<string | null>(null);

/** 日志展示内容：倒序（最新一行在最上面，向下越来越旧） */
const displayAppLog = computed(() => {
  if (!appLog.value) return "";
  return appLog.value
    .split(/\r?\n/)
    .reverse()
    .join("\n");
});

async function loadAppLog() {
  appLogLoading.value = true;
  appLogError.value = null;
  try {
    appLog.value = await debugApi.readAppLog(200_000);
  } catch (e) {
    appLogError.value = e instanceof Error ? e.message : String(e);
    appLog.value = "";
  } finally {
    appLogLoading.value = false;
  }
}

async function clearLogs() {
  if (!confirm("确定清空应用日志文件（ai-debug.log）吗？此操作不可恢复。")) return;
  try {
    await debugApi.clearAppLog();
    appLog.value = "";
    appLogError.value = null;
  } catch (e) {
    appLogError.value = e instanceof Error ? e.message : String(e);
  }
}

onMounted(loadAppLog);
</script>

<template>
  <Card id="debug-logs" padding="lg" class="debug-section">
    <div class="section-head">
      <div class="section-title">
        <ScrollText :size="18" />
        <span>日志查看</span>
      </div>
      <div class="section-actions-inline">
        <Button variant="ghost" size="sm" :loading="appLogLoading" @click="loadAppLog">
          <RefreshCw :size="14" />
          <span>刷新</span>
        </Button>
        <Button variant="ghost" size="sm" @click="clearLogs">
          <Trash2 :size="14" />
          <span>清除</span>
        </Button>
      </div>
    </div>

    <p class="section-desc">
      展示 AI 调试日志（logs/ai-debug.log）的内容，最新在最上面、向下越旧，包含 AI 请求/响应记录、后端 warn/error 等。结构化调用详情见上方「AI 调用记录」与「AI 用量」模块。
    </p>

    <div v-if="appLogError" class="error-text">{{ appLogError }}</div>
    <div v-else-if="!appLog" class="empty-inline">暂无日志。</div>
    <pre v-else class="app-log-view">{{ displayAppLog }}</pre>
  </Card>
</template>
