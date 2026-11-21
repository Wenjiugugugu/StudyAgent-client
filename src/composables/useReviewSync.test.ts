import { expect, it, vi } from "vitest";
import * as api from "@/api";
import { useReviewSync } from "./useReviewSync";
import type { ReviewSyncStatus } from "@/types/review";
vi.mock("@/api", () => ({ getReviewSyncStatus: vi.fn(), retryReviewSync: vi.fn() }));
function deferred() {
  let resolve!: (value: ReviewSyncStatus) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<ReviewSyncStatus>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function status(revision: string): ReviewSyncStatus {
  return {
    review_revision: revision,
    sync_status: "pending",
    pending_steps: ["state"],
    warnings: [],
  };
}
it("旧读取结果不覆盖刚保存的新复盘状态", async () => {
  const pending = deferred();
  vi.mocked(api.getReviewSyncStatus).mockReturnValueOnce(pending.promise);
  const controller = useReviewSync(() => "2026-10-13", vi.fn());
  const loading = controller.load();
  controller.record(status("saved"));
  pending.resolve(status("old"));
  await loading;
  expect(controller.status.value?.review_revision).toBe("saved");
});
it("切换日期后丢弃旧重试结果及旧错误", async () => {
  let date = "2026-10-12";
  const error = vi.fn(),
    pending = deferred();
  vi.mocked(api.retryReviewSync).mockReturnValueOnce(pending.promise);
  const controller = useReviewSync(() => date, error);
  controller.record(status("old"));
  const retry = controller.retry();
  date = "2026-10-13";
  controller.record(status("new"));
  pending.reject(new Error("旧日期错误"));
  await retry;
  expect(error).not.toHaveBeenCalled();
  expect(controller.status.value?.review_revision).toBe("new");
  expect(controller.retrying.value).toBe(false);
});
it("卸载后忽略尚未返回的状态请求", async () => {
  const pending = deferred();
  vi.mocked(api.getReviewSyncStatus).mockReturnValueOnce(pending.promise);
  const controller = useReviewSync(() => "2026-10-13", vi.fn());
  const loading = controller.load();
  controller.dispose();
  pending.resolve(status("late"));
  await loading;
  expect(controller.status.value).toBeNull();
});
