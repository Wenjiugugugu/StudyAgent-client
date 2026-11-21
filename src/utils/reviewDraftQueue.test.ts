import { expect, it, vi } from "vitest";
import { createReviewDraftQueue } from "./reviewDraftQueue";
it("失败传给调用方，后续保存保留原日期与捕获内容", async () => {
  const persist = vi.fn().mockRejectedValueOnce(new Error("磁盘失败")).mockResolvedValue(undefined);
  const queue = createReviewDraftQueue<{ date: string; text: string }>(persist);
  await expect(queue.save({ date: "2026-10-11", text: "原文" })).rejects.toThrow("磁盘失败");
  const draft = { date: "2026-10-12", text: "确认内容" };
  const pending = queue.save(draft);
  draft.text = "后来的编辑";
  await pending;
  expect(persist).toHaveBeenLastCalledWith({ date: "2026-10-12", text: "确认内容" });
});
it("卸载后取消自动保存，重新挂载的队列可独立工作", async () => {
  vi.useFakeTimers();
  const persist = vi.fn().mockResolvedValue(undefined);
  const queue = createReviewDraftQueue(persist);
  const action = vi.fn();
  queue.schedule(action);
  queue.dispose();
  vi.runAllTimers();
  expect(action).not.toHaveBeenCalled();
  await expect(queue.save({ text: "旧页面" })).rejects.toThrow("已离开");
  const next = createReviewDraftQueue(persist);
  await next.save({ text: "新页面" });
  expect(persist).toHaveBeenCalledOnce();
  vi.useRealTimers();
});
