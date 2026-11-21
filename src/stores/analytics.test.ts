import { beforeEach, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useAnalyticsStore } from "./analytics";
import * as api from "@/api";
import type { AnalyticsSummary } from "@/types";
vi.mock("@/api", () => ({ getAnalytics: vi.fn() }));
function deferred() {
  let resolve!: (value: AnalyticsSummary) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<AnalyticsSummary>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
beforeEach(() => {
  setActivePinia(createPinia());
  vi.clearAllMocks();
});
it("旧结果和旧错误都不能覆盖新筛选", async () => {
  const a = deferred(),
    b = deferred();
  vi.mocked(api.getAnalytics).mockReturnValueOnce(a.promise).mockReturnValueOnce(b.promise);
  const store = useAnalyticsStore();
  const first = store.load("last_7_days"),
    second = store.load("all");
  const latest = { learning_trend: {} } as AnalyticsSummary;
  b.resolve(latest);
  await second;
  a.reject(new Error("旧错误"));
  await first;
  expect(store.currentRange).toBe("all");
  expect(store.summary).toEqual(latest);
  expect(store.error).toBeNull();
});
it("旧请求结束时新请求仍在加载，最终保留新结果", async () => {
  const a = deferred(),
    b = deferred();
  vi.mocked(api.getAnalytics).mockReturnValueOnce(a.promise).mockReturnValueOnce(b.promise);
  const store = useAnalyticsStore();
  const first = store.load("last_7_days"),
    second = store.load("all");
  a.resolve({} as AnalyticsSummary);
  await first;
  expect(store.loading).toBe(true);
  expect(store.summary).toBeNull();
  b.resolve({ learning_trend: {} } as AnalyticsSummary);
  await second;
  expect(store.loading).toBe(false);
});
