/**
 * useAiRequest — AI 调用状态统一管理 composable（状态管理统一化重构，2026-08-04）
 *
 * 解决 code review H21/H23/H31/L61：各 store/view 手写 loading/error 三件套、
 * 有的缺 error 字段、错误展示不一致的问题。
 *
 * 用法：
 * ```ts
 * const { pending, error, run } = useAiRequest();
 * async function generate() {
 *   const result = await run(() => api.generateWeekPlan(...), "生成周计划失败");
 *   if (result) { ... }
 * }
 * ```
 *
 * 约定：
 * - `pending`：调用进行中（驱动按钮 loading / 卡片骨架）
 * - `error`：最近一次失败的错误信息（null 表示无错误），视图层统一渲染
 * - `run(fn, errorPrefix?)`：执行调用；成功返回结果并清空 error，失败记录错误并 rethrow
 * - 支持多个并发调用：每个 `run` 独立计数，pending 为 true 当且仅当存在进行中的调用
 * - 组件卸载后不再更新状态（防止 H30 类「卸载后 set state」警告）
 */
import { onScopeDispose, ref, shallowRef } from "vue";

export function useAiRequest() {
  const pending = ref(false);
  const error = ref<string | null>(null);
  /** 计数器：支持并发 run，全部结束后 pending 才归 false */
  const inflight = shallowRef(0);

  onScopeDispose(() => {
    inflight.value = 0;
    pending.value = false;
  });

  function setPending(v: boolean) {
    inflight.value = v ? inflight.value + 1 : Math.max(0, inflight.value - 1);
    pending.value = inflight.value > 0;
  }

  async function run<T>(fn: () => Promise<T>, errorPrefix?: string): Promise<T | null> {
    setPending(true);
    try {
      const result = await fn();
      error.value = null;
      return result;
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      error.value = errorPrefix ? `${errorPrefix}：${message}` : message;
      throw e;
    } finally {
      setPending(false);
    }
  }

  function clearError() {
    error.value = null;
  }

  /** 手动设置错误（非 run 路径的调用失败时使用，如普通读取） */
  function setError(message: string | null) {
    error.value = message;
  }

  return { pending, error, run, clearError, setError };
}
