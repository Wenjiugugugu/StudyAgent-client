/**
 * 调试页 — AI Provider 诊断逻辑（原 DebugView「AI Provider 测试」区块）
 */
import { ref } from "vue";
import { debugApi } from "../api";
import type { ProviderTestState } from "../types";

export function useProviderDiagnostics() {
  const providerTests = ref<ProviderTestState[]>([]);

  async function loadProviders() {
    try {
      const s = await debugApi.getSettings();
      providerTests.value = (s.ai_providers ?? []).map((p) => ({
        provider: p,
        status: "idle" as const,
        message: "",
      }));
    } catch {
      providerTests.value = [];
    }
  }

  async function testProvider(idx: number) {
    const item = providerTests.value[idx];
    if (!item) return;
    item.status = "loading";
    item.message = "";
    try {
      const result = await debugApi.testAIProvider(item.provider);
      item.status = result.success ? "success" : "error";
      item.message = result.message;
    } catch (e) {
      item.status = "error";
      item.message = e instanceof Error ? e.message : String(e);
    }
  }

  async function testAllProviders() {
    // H36：并行测试所有 provider，避免串行 N×latency 阻塞 UI
    const tests = providerTests.value.map((_, i) => testProvider(i));
    await Promise.allSettled(tests);
  }

  return {
    providerTests,
    loadProviders,
    testProvider,
    testAllProviders,
  };
}
