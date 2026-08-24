<script setup lang="ts">
/**
 * 调试页 — Settings 查看面板
 *
 * 读取当前设置并展示 JSON；同时把 data_directory 上报给父页面，
 * 供系统信息 / 数据目录检查等面板使用。
 */
import { ref, computed } from "vue";
import Card from "@/components/ui/Card.vue";
import Button from "@/components/ui/Button.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import { Settings2, RefreshCw } from "lucide-vue-next";
import { debugApi } from "../api";
import { formatJson } from "../utils/json";
import type { TestResult } from "../types";
import type { AppSettings } from "@/types";

const emit = defineEmits<{ (e: "data-dir-change", dir: string): void }>();

const settingsView = ref<TestResult<AppSettings>>({ status: "idle", data: null, error: null });
const configPath = computed(() => settingsView.value.data?.data_directory ?? "");

async function loadSettingsView() {
  settingsView.value = { status: "loading", data: null, error: null };
  try {
    const data = await debugApi.getSettings();
    settingsView.value = { status: "success", data, error: null };
    // 后端 Rust 字段名为 data_dir，前端类型为 data_directory，兼容两者
    const s = data as AppSettings & { data_dir?: string };
    emit("data-dir-change", s?.data_dir || s?.data_directory || "");
  } catch (e) {
    settingsView.value = {
      status: "error",
      data: null,
      error: e instanceof Error ? e.message : String(e),
    };
  }
}

defineExpose({ refresh: loadSettingsView });
</script>

<template>
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
</template>
