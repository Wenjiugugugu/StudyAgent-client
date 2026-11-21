import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useTodayStore } from "./today";
import * as api from "@/api";
import type { DailyPlan } from "@/types";

vi.mock("@/api", () => ({ getPlanByDate: vi.fn() }));

function deferred() {
  let resolve!: (value: DailyPlan) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<DailyPlan>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function plan(date: string) {
  return { meta: { date } } as DailyPlan;
}
beforeEach(() => {
  setActivePinia(createPinia());
  vi.clearAllMocks();
});

describe("日期加载以最新请求为准", () => {
  it("旧请求晚返回，不覆盖新的日期", async () => {
    const a = deferred(),
      b = deferred();
    vi.mocked(api.getPlanByDate).mockReturnValueOnce(a.promise).mockReturnValueOnce(b.promise);
    const store = useTodayStore();
    const first = store.loadByDate("2020-01-01"),
      second = store.loadByDate("2020-01-02");
    b.resolve(plan("2020-01-02"));
    await second;
    a.resolve(plan("2020-01-01"));
    await first;
    expect(store.plan?.meta.date).toBe("2020-01-02");
  });
  it("旧请求失败，不清空已加载的新计划", async () => {
    const a = deferred(),
      b = deferred();
    vi.mocked(api.getPlanByDate).mockReturnValueOnce(a.promise).mockReturnValueOnce(b.promise);
    const store = useTodayStore();
    const first = store.loadByDate("2020-01-01"),
      second = store.loadByDate("2020-01-02");
    b.resolve(plan("2020-01-02"));
    await second;
    a.reject(new Error("旧请求失败"));
    await first;
    expect(store.plan?.meta.date).toBe("2020-01-02");
    expect(store.error).toBeNull();
  });
  it("旧请求结束，仍保留新请求的加载状态", async () => {
    const a = deferred(),
      b = deferred();
    vi.mocked(api.getPlanByDate).mockReturnValueOnce(a.promise).mockReturnValueOnce(b.promise);
    const store = useTodayStore();
    const first = store.loadByDate("2020-01-01"),
      second = store.loadByDate("2020-01-02");
    a.resolve(plan("2020-01-01"));
    await first;
    expect(store.loading).toBe(true);
    b.resolve(plan("2020-01-02"));
    await second;
    expect(store.loading).toBe(false);
  });
});
