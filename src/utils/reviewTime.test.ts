import { describe, expect, it } from "vitest";
import { reviewTaskMinutes } from "./reviewTime";
import type { FocusSession } from "@/api";
import type { StudyState } from "@/types";

describe("复盘时长回填", () => {
  it("已完成番茄不重复累加，打断的学习也保留", () => {
    const sessions = [
      { task_id: "task", type: "focus", status: "completed", duration_minutes: 25 },
      { task_id: "task", type: "stopwatch", status: "interrupted", duration_minutes: 5 },
      { task_id: "task", type: "short_break", status: "completed", duration_minutes: 10 },
    ] as FocusSession[];
    const state = {
      current_task: { date: "2026-10-06", tasks: [{ task_id: "task", accumulated_minutes: 40 }] },
    } as StudyState;
    expect(reviewTaskMinutes("2026-10-06", sessions, state)).toEqual({ task: 45 });
    expect(reviewTaskMinutes("2026-10-05", sessions, state)).toEqual({ task: 30 });
  });
  it("没有记录时不制造零分钟；明确的零分钟记录会保留", () => {
    expect(reviewTaskMinutes("2026-10-06", [])).toEqual({});
    expect(
      reviewTaskMinutes("2026-10-06", [
        { task_id: "task", type: "focus", status: "completed", duration_minutes: 0 },
      ] as FocusSession[])
    ).toEqual({ task: 0 });
  });
});
