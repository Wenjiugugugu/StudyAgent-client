/**
 * 调试页 — 统一刷新编排（原 DebugView「刷新全部」逻辑）
 *
 * 只负责并发执行各面板的 refresh 任务并收集逐项错误；每个子项自带
 * try/catch 记录独立错误，任一失败不阻塞其余调用（H42）。
 */
import { ref } from "vue";

export function useDebugRefresh() {
  const refreshing = ref(false);
  const refreshErrors = ref<string[]>([]);

  /**
   * 并发执行一组刷新任务，收集 rejected 结果并写入 refreshErrors。
   * tasks 返回的 Promise 均应在内部消化异常（resolve 成功），
   * 只有真正未捕获的异常才会被记为「刷新失败」。
   */
  async function refreshAll(tasks: Array<() => Promise<unknown>>) {
    refreshing.value = true;
    refreshErrors.value = [];
    const results = await Promise.allSettled(tasks.map((t) => t()));
    results.forEach((r, i) => {
      if (r.status === "rejected") {
        refreshErrors.value.push(`第 ${i + 1} 项刷新失败：${String(r.reason)}`);
      }
    });
    refreshing.value = false;
  }

  /** 追加一条刷新错误提示（供需要 dataDir 的后续任务使用） */
  function pushRefreshError(message: string) {
    refreshErrors.value.push(message);
  }

  return {
    refreshing,
    refreshErrors,
    refreshAll,
    pushRefreshError,
  };
}
