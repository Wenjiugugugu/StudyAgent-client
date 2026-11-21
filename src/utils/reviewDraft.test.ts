import { describe, expect, it } from "vitest";
import {
  matchingDraftTasks,
  reviewTaskSignatures,
  isReviewDraftForm,
  type ReviewDraftForm,
} from "./reviewDraft";
import type { PlanTask } from "@/types";

describe("草稿恢复时核对任务身份", () => {
  it("同编号任务改名或知识点改变时不能继承原勾选", () => {
    const old = [
      {
        id: "a",
        subject: "math",
        title: "第一章",
        knowledge_points: [{ table_id: "math", node_id: "1" }],
      },
      { id: "b", subject: "english", title: "词汇" },
    ] as PlanTask[];
    const form = {
      taskSignatures: reviewTaskSignatures(old),
      mainDifficulty: "保留这段文字",
    } as ReviewDraftForm;
    expect(matchingDraftTasks(form, old)).toEqual(["a", "b"]);
    expect(matchingDraftTasks(form, [{ ...old[0], title: "第二章" }, old[1]])).toEqual(["b"]);
    expect(matchingDraftTasks(form, [{ ...old[0], knowledge_points: [] }, old[1]])).toEqual(["b"]);
    expect(form.mainDifficulty).toBe("保留这段文字");
  });
  it("字段不完整的草稿不能部分覆盖表单", () => {
    expect(isReviewDraftForm({ mainDifficulty: "旧内容" })).toBe(false);
    expect(isReviewDraftForm(null)).toBe(false);
  });
});
