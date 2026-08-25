<script setup lang="ts">
/**
 * 调试页 — Dashboard 数据测试面板
 */
import { ref, computed } from "vue";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import { LayoutDashboard, RefreshCw } from "lucide-vue-next";
import { debugApi } from "../api";
import { formatJson } from "../utils/json";
import { statusBadge, statusLabel } from "../utils/status";
import type { TestResult } from "../types";
import type { DashboardSummary } from "@/types";

const dashboardTest = ref<TestResult<DashboardSummary>>({ status: "idle", data: null, error: null });

async function runDashboardTest() {
  dashboardTest.value = { status: "loading", data: null, error: null };
  try {
    const data = await debugApi.getDashboardSummary();
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

defineExpose({ refresh: runDashboardTest });
</script>

<template>
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
</template>
