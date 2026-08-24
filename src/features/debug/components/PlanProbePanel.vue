<script setup lang="ts">
/**
 * 调试页 — Plan 解析测试面板
 */
import { ref, computed } from "vue";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import { Calendar, RefreshCw } from "lucide-vue-next";
import { debugApi } from "../api";
import { formatJson } from "../utils/json";
import { statusBadge, statusLabel } from "../utils/status";
import type { TestResult } from "../types";
import type { DailyPlan } from "@/types";
import { todayString } from "@/utils/date";

const planTest = ref<TestResult<DailyPlan>>({ status: "idle", data: null, error: null });
const planTestDate = ref(todayString());

async function runPlanTest() {
  planTest.value = { status: "loading", data: null, error: null };
  try {
    const data = await debugApi.getPlanByDate(planTestDate.value);
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

defineExpose({ refresh: runPlanTest });
</script>

<template>
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
</template>
