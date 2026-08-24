<script setup lang="ts">
/**
 * 调试页 — AI 用量日志面板
 *
 * 展示持久化的 AI 用量记录（含费用估算）、时间筛选、汇总统计与调用明细。
 */
import { ref } from "vue";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import { Coins, RefreshCw, Trash2, ChevronRight, ChevronLeft } from "lucide-vue-next";
import { formatTokens, formatCost, fetchLatestPricingNote } from "@/utils/aiPricing";
import { debugApi } from "../api";
import { useUsageFilters, usageEntryKey } from "../composables/useUsageFilters";
import { formatUsageDuration, formatUsageTimestamp, agentLabel } from "../utils/formatters";
import { usageStatusBadge, usageStatusLabel } from "../utils/status";
import type { AiUsageEntry } from "@/types";

const aiUsageLog = ref<AiUsageEntry[]>([]);
const aiUsageLoading = ref(false);
const aiUsageError = ref<string | null>(null);

const {
  usageTimeFilter,
  filteredUsageLog,
  usagePage,
  usagePageCount,
  pagedUsageLog,
  usagePageStart,
  expandedUsageIdx,
  usageCostMap,
  usageSummary,
  toggleUsageEntry,
} = useUsageFilters(aiUsageLog);

/** 定价表更新提示 */
const pricingNote = fetchLatestPricingNote();

async function loadAiUsageLog() {
  aiUsageLoading.value = true;
  aiUsageError.value = null;
  try {
    aiUsageLog.value = await debugApi.getAiUsageLog();
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
    await debugApi.clearAiUsageLog();
    aiUsageLog.value = [];
    expandedUsageIdx.value = null;
  } catch (e) {
    aiUsageError.value = e instanceof Error ? e.message : String(e);
  }
}

defineExpose({ refresh: loadAiUsageLog });
</script>

<template>
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
        v-for="(entry, i) in pagedUsageLog"
        :key="usagePageStart + i"
        class="usage-item"
        :class="{ expanded: expandedUsageIdx === usagePageStart + i }"
      >
        <button class="usage-item-header" @click="toggleUsageEntry(usagePageStart + i)">
          <ChevronRight :size="14" class="ai-call-chevron" :class="{ open: expandedUsageIdx === usagePageStart + i }" />
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

        <div v-if="expandedUsageIdx === usagePageStart + i" class="usage-item-detail">
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
      <div class="pagination">
        <button
          class="pagination-btn"
          :disabled="usagePage <= 1"
          title="上一页"
          @click="usagePage--"
        >
          <ChevronLeft :size="14" />
        </button>
        <span class="pagination-info">第 {{ usagePage }} / {{ usagePageCount }} 页</span>
        <button
          class="pagination-btn"
          :disabled="usagePage >= usagePageCount"
          title="下一页"
          @click="usagePage++"
        >
          <ChevronRight :size="14" />
        </button>
      </div>
    </div>

    <div v-if="!aiUsageLoading && filteredUsageLog.length === 0 && !aiUsageError" class="empty-inline">
      {{ aiUsageLog.length === 0 ? "暂无 AI 用量记录。生成计划、生成复盘或在助手页发送对话后会显示在此。" : "当前筛选条件下无记录。" }}
    </div>
  </Card>
</template>
