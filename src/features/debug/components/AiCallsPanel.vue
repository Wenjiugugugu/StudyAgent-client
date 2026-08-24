<script setup lang="ts">
/**
 * 调试页 — AI 调用记录面板
 *
 * 实时展示 store 中记录的 AI 调用（请求/响应/推理过程/耗时/错误），
 * 每 10 条一页，数据源为 useAiDebugStore（内存 + localStorage 持久化）。
 */
import { ref, computed, watch } from "vue";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import {
  Radio,
  Trash2,
  ChevronRight,
  ChevronLeft,
  Send,
  CheckCircle2,
  Brain,
  AlertTriangle,
} from "lucide-vue-next";
import { useAiDebugStore } from "@/stores/aiDebug";
import { formatJson } from "../utils/json";
import { formatDuration, formatTimestamp } from "../utils/formatters";
import { aiCallStatusBadge, aiCallStatusLabel } from "../utils/status";

/** 每页记录条数 */
const RECORDS_PAGE_SIZE = 10;
const aiDebugStore = useAiDebugStore();
/** 当前展开查看详情的记录 ID（null 表示全部折叠） */
const expandedAiCallId = ref<number | null>(null);
/** AI 调用记录分页：当前页码 */
const recordsPage = ref(1);
const recordsPageCount = computed(() =>
  Math.max(1, Math.ceil(aiDebugStore.records.length / RECORDS_PAGE_SIZE)),
);
/** 当前页展示的记录（最新在前） */
const pagedAICalls = computed(() => {
  const start = (recordsPage.value - 1) * RECORDS_PAGE_SIZE;
  return aiDebugStore.records.slice(start, start + RECORDS_PAGE_SIZE);
});
// 记录数变化（新增/清空）时把页码收敛到有效范围内
watch(
  () => aiDebugStore.records.length,
  () => {
    if (recordsPage.value > recordsPageCount.value) {
      recordsPage.value = recordsPageCount.value;
    }
  },
);

function toggleAiCall(id: number) {
  expandedAiCallId.value = expandedAiCallId.value === id ? null : id;
}
</script>

<template>
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
      实时记录所有 AI 调用：请求参数、响应数据、AI 的思考过程（推理模型）、耗时与错误，每 10 条为一页；
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
        v-for="rec in pagedAICalls"
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

          <div v-if="rec.status === 'success' && rec.reasoning" class="ai-call-block">
            <div class="ai-call-block-head">
              <Brain :size="13" />
              <span>思考过程（推理模型）</span>
            </div>
            <pre class="code-block reasoning-block">{{ rec.reasoning }}</pre>
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
      <div class="pagination">
        <button
          class="pagination-btn"
          :disabled="recordsPage <= 1"
          title="上一页"
          @click="recordsPage--"
        >
          <ChevronLeft :size="14" />
        </button>
        <span class="pagination-info">第 {{ recordsPage }} / {{ recordsPageCount }} 页</span>
        <button
          class="pagination-btn"
          :disabled="recordsPage >= recordsPageCount"
          title="下一页"
          @click="recordsPage++"
        >
          <ChevronRight :size="14" />
        </button>
      </div>
    </div>
  </Card>
</template>
