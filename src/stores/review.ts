import { defineStore } from "pinia";
import { ref } from "vue";
import * as api from "@/api";
import { todayString } from "@/utils/date";
import type { ReviewRecord } from "@/types";

export const useReviewStore = defineStore("review", () => {
  const current = ref<ReviewRecord | null>(null);
  const loading = ref(false);
  const generating = ref(false);
  const error = ref<string | null>(null);

  async function loadReview(date: string) {
    loading.value = true;
    error.value = null;
    current.value = null;
    try {
      current.value = await api.getReview(date);
    } catch (e) {
      current.value = null;
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      loading.value = false;
    }
  }

  async function generateReview(date: string) {
    generating.value = true;
    error.value = null;
    try {
      // 仅允许生成今天的复盘
      if (date !== todayString()) {
        throw new Error("只能生成今天的学习复盘，不支持生成过去或未来的复盘");
      }

      // 增加 60 秒 UI 层超时保护，避免 AI 请求挂起时一直转圈
      current.value = await withTimeout(
        api.generateReview(date),
        60000,
        `生成复盘超时（超过 60 秒）。请检查 AI Provider 配置或网络连接。`
      );
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      generating.value = false;
    }
  }

  function withTimeout<T>(promise: Promise<T>, ms: number, timeoutMessage: string): Promise<T> {
    return Promise.race([
      promise,
      new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error(timeoutMessage)), ms)
      ),
    ]);
  }

  return { current, loading, generating, error, loadReview, generateReview };
});
