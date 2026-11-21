import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { nextTick } from "vue";
import { useFocusStore } from "./focus";
import * as api from "@/api";
import { formatLocalIso } from "@/utils/date";

vi.mock("@/api", () => ({
  focusAddMinutes: vi.fn(),
  recordFocusSession: vi.fn().mockResolvedValue(undefined),
  getFocusTodayStats: vi.fn().mockResolvedValue({ pomodoros: 0, focus_minutes: 0, breaks: 0 }),
  getFocusSessions: vi.fn().mockResolvedValue([]),
  getFocusSessionsRange: vi.fn().mockResolvedValue([]),
  isTauri: vi.fn().mockReturnValue(false),
}));

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-10-07T10:00:00+08:00"));
  vi.stubGlobal("localStorage", { getItem: vi.fn().mockReturnValue(null), setItem: vi.fn() });
  setActivePinia(createPinia());
  vi.clearAllMocks();
});

afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("专注会话保存", () => {
  it("更新准备等待正在保存的会话，重试记录时不重复累计任务分钟", async () => {
    vi.mocked(api.focusAddMinutes).mockResolvedValue(undefined);
    vi.mocked(api.recordFocusSession).mockRejectedValueOnce(new Error("暂时无法写入"));
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const store = useFocusStore();
    store.linkedTaskId = "task-a";
    store.startStopwatch();
    vi.advanceTimersByTime(60_000);
    store.finishStopwatch();
    await store.prepareForUpdate();
    expect(api.focusAddMinutes).toHaveBeenCalledTimes(1);
    expect(api.recordFocusSession).toHaveBeenCalledTimes(2);
    expect(vi.mocked(api.recordFocusSession).mock.calls[1][0]).toEqual(
      vi.mocked(api.recordFocusSession).mock.calls[0][0]
    );
    vi.restoreAllMocks();
  });
  it("更新前保存暂停状态，重启后不计入安装期间时长", async () => {
    const store = useFocusStore();
    store.startStopwatch();
    vi.advanceTimersByTime(90_000);
    await store.prepareForUpdate();
    const writes = vi.mocked(localStorage.setItem).mock.calls;
    const stateWrites = writes.filter(([key]) => key === "focus.state.v1");
    const saved = JSON.parse(stateWrites[stateWrites.length - 1][1]);
    expect(saved.sub).toBe("paused");
    expect(saved.stopwatchBaseSec).toBe(90);
    vi.mocked(localStorage.getItem).mockImplementation((key) =>
      key === "focus.state.v1" ? JSON.stringify(saved) : null
    );
    setActivePinia(createPinia());
    vi.advanceTimersByTime(120_000);
    const restored = useFocusStore();
    expect(restored.isPaused).toBe(true);
    expect(restored.stopwatchSec).toBe(90);
    expect(api.recordFocusSession).not.toHaveBeenCalled();
  });

  it("更新准备中的持久化失败必须透传", async () => {
    const store = useFocusStore();
    vi.mocked(localStorage.setItem).mockImplementation(() => {
      throw new Error("存储不可用");
    });
    await expect(store.prepareForUpdate()).rejects.toThrow("存储不可用");
  });
  it("保存期间切换关联任务，不改变已结束会话的任务和时间", async () => {
    let resolveCredit!: () => void;
    vi.mocked(api.focusAddMinutes).mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          resolveCredit = resolve;
        })
    );
    const store = useFocusStore();
    store.linkedTaskId = "task-a";
    const started = Date.now();
    store.startStopwatch();
    vi.advanceTimersByTime(60_000);
    const ended = Date.now();
    store.finishStopwatch();
    expect(api.focusAddMinutes).toHaveBeenCalledWith("task-a", 1);
    store.linkedTaskId = "task-b";
    store.startStopwatch();
    vi.advanceTimersByTime(5_000);
    resolveCredit();
    await nextTick();
    expect(api.recordFocusSession).toHaveBeenCalledWith(
      expect.objectContaining({
        task_id: "task-a",
        started_at: formatLocalIso(new Date(started)),
        ended_at: formatLocalIso(new Date(ended)),
        task_minutes_recorded: true,
      })
    );
  });

  it("累计失败仍保存会话，并明确它尚未计入任务时长", async () => {
    vi.mocked(api.focusAddMinutes).mockRejectedValue(new Error("写入失败"));
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const store = useFocusStore();
    store.linkedTaskId = "task-a";
    store.startStopwatch();
    vi.advanceTimersByTime(60_000);
    store.finishStopwatch();
    await nextTick();
    expect(api.recordFocusSession).toHaveBeenCalledWith(
      expect.objectContaining({
        task_id: "task-a",
        task_minutes_recorded: false,
        duration_minutes: 1,
      })
    );
    vi.restoreAllMocks();
  });
});
