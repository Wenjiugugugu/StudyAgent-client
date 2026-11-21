import { expect, it, vi } from "vitest";
import { createReviewNavigation } from "./reviewNavigation";
it("保存失败不切换日期或清空编辑", async () => {
  const commit = vi.fn(),
    error = vi.fn();
  const navigate = createReviewNavigation({
    currentDate: () => "2026-10-11",
    busy: () => false,
    saveDraft: async () => {
      throw new Error("保存失败");
    },
    commit,
    onError: error,
  });
  expect(await navigate("2026-10-12")).toBe(false);
  expect(commit).not.toHaveBeenCalled();
  expect(error).toHaveBeenCalledOnce();
});
it("等待当前日期保存完成，重复点击不能发起第二次切换", async () => {
  let saved!: () => void;
  const commit = vi.fn();
  const save = vi.fn(
    () =>
      new Promise<void>((resolve) => {
        saved = resolve;
      })
  );
  const navigate = createReviewNavigation({
    currentDate: () => "2026-10-11",
    busy: () => false,
    saveDraft: save,
    commit,
    onError: vi.fn(),
  });
  const first = navigate("2026-10-12");
  expect(await navigate("2026-10-13")).toBe(false);
  expect(commit).not.toHaveBeenCalled();
  saved();
  expect(await first).toBe(true);
  expect(commit).toHaveBeenCalledWith("2026-10-12", "jump");
  expect(save).toHaveBeenCalledOnce();
});
