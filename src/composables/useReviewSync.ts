import { ref } from "vue";
import * as api from "@/api";
import type { ReviewSyncStatus } from "@/types/review";

/** 保存结果、日期加载和重试共用版本检查，旧请求不能覆盖新复盘。 */
export function useReviewSync(currentDate: () => string, onError: (message: string) => void) {
  const status = ref<ReviewSyncStatus | null>(null);
  const retrying = ref(false);
  let request = 0;
  let disposed = false;
  function record(result: ReviewSyncStatus) {
    if (disposed) return;
    request++;
    status.value = result;
  }
  async function load() {
    if (disposed) return;
    const date = currentDate(),
      id = ++request;
    status.value = null;
    const current = () => !disposed && request === id && currentDate() === date;
    try {
      const result = await api.getReviewSyncStatus(date);
      if (current()) status.value = result;
    } catch (error) {
      if (current()) onError(`同步状态读取失败：${String(error)}`);
    }
  }
  async function retry() {
    if (disposed || !status.value || retrying.value) return;
    const date = currentDate(),
      revision = status.value.review_revision,
      id = ++request;
    retrying.value = true;
    const current = () => !disposed && request === id && currentDate() === date;
    try {
      const result = await api.retryReviewSync(date, revision);
      if (current()) status.value = result;
    } catch (error) {
      if (current()) onError(String(error));
    } finally {
      retrying.value = false;
    }
  }
  function dispose() {
    disposed = true;
    request++;
  }
  return { status, retrying, record, load, retry, dispose };
}
