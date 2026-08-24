/**
 * 调试页 — AI 用量筛选 / 分页 / 汇总统计（原 DebugView「AI 用量日志」的计算部分）
 *
 * 纯状态与计算逻辑，加载与清空动作由面板组件持有。
 */
import { computed, ref, watch, type Ref } from "vue";
import { estimateCost, type CostEstimate } from "@/utils/aiPricing";
import type { AiUsageEntry } from "@/types";

/** 时间筛选维度 */
export type UsageTimeFilter = "all" | "today" | "7d" | "30d";

/** 用量「调用明细」每页条数 */
const USAGE_PAGE_SIZE = 10;

/** 单条记录的费用估算（缓存以避免重复计算） */
export function usageEntryKey(entry: AiUsageEntry): string {
  return `${entry.timestamp}|${entry.model}|${entry.prompt_tokens}|${entry.completion_tokens}`;
}

export function useUsageFilters(aiUsageLog: Ref<AiUsageEntry[]>) {
  const expandedUsageIdx = ref<number | null>(null);
  const usageTimeFilter = ref<UsageTimeFilter>("all");

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

  /** 用量「调用明细」分页：当前页码 */
  const usagePage = ref(1);
  const usagePageCount = computed(() =>
    Math.max(1, Math.ceil(filteredUsageLog.value.length / USAGE_PAGE_SIZE)),
  );
  /** 当前页展示的用量明细（最新在前） */
  const pagedUsageLog = computed(() => {
    const start = (usagePage.value - 1) * USAGE_PAGE_SIZE;
    return filteredUsageLog.value.slice(start, start + USAGE_PAGE_SIZE);
  });
  /** 当前页首页在完整明细里的下标（用于展开/收起的定位） */
  const usagePageStart = computed(() => (usagePage.value - 1) * USAGE_PAGE_SIZE);
  // 明细条数变化时把页码收敛到有效范围内
  watch(
    () => filteredUsageLog.value.length,
    () => {
      if (usagePage.value > usagePageCount.value) {
        usagePage.value = usagePageCount.value;
      }
    },
  );

  /** 单条记录的费用估算（缓存以避免重复计算） */
  const usageCostMap = computed<Map<string, CostEstimate>>(() => {
    const map = new Map<string, CostEstimate>();
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

  function toggleUsageEntry(idx: number) {
    expandedUsageIdx.value = expandedUsageIdx.value === idx ? null : idx;
  }

  return {
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
  };
}
