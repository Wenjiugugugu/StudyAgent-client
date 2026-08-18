import { defineStore } from "pinia";
import { ref } from "vue";
import * as api from "@/api";
import { todayString } from "@/utils/date";
import { useAiRequest } from "@/composables/useAiRequest";
import type { ReviewRecord } from "@/types";

export const useReviewStore = defineStore("review", () => {
  const current = ref<ReviewRecord | null>(null);
  const loading = ref(false);
  // 统一 AI 请求状态（H21：替代手写 withTimeout + generating 三件套）
  const ai = useAiRequest();
  const generating = ai.pending;
  const error = ai.error;

  async function loadReview(date: string) {
    loading.value = true;
    ai.clearError();
    current.value = null;
    try {
      current.value = await api.getReview(date);
    } catch (e) {
      current.value = null;
      ai.setError(e instanceof Error ? e.message : String(e));
    } finally {
      loading.value = false;
    }
  }

  async function generateReview(date: string) {
    try {
      // 仅允许生成今天的复盘
      if (date !== todayString()) {
        throw new Error("只能生成今天的学习复盘，不支持生成过去或未来的复盘");
      }

      // 统一 AI 调用：api.generateReview 内部已含 300s 超时 + 自动取消（aiInvoke）
      await ai.run(
        () => api.generateReview(date).then((r) => { current.value = r; }),
        "生成复盘失败"
      );
    } catch (e) {
      // useAiRequest 已记录 error，此处不再重复赋值
    }
  }

  return { current, loading, generating, error, loadReview, generateReview };
});
