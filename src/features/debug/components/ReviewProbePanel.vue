<script setup lang="ts">
/**
 * 调试页 — Review 解析测试面板
 */
import { ref, computed } from "vue";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import { FileCheck, RefreshCw } from "lucide-vue-next";
import { debugApi } from "../api";
import { formatJson } from "../utils/json";
import { statusBadge, statusLabel } from "../utils/status";
import type { TestResult } from "../types";
import type { ReviewRecord } from "@/types";
import { todayString } from "@/utils/date";

const reviewTest = ref<TestResult<ReviewRecord>>({ status: "idle", data: null, error: null });
const reviewTestDate = ref(todayString());

async function runReviewTest() {
  reviewTest.value = { status: "loading", data: null, error: null };
  try {
    const data = await debugApi.getReview(reviewTestDate.value);
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

defineExpose({ refresh: runReviewTest });
</script>

<template>
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
</template>
