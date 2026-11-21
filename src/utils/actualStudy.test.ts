import { describe, expect, it } from "vitest";
import { actualStudyError } from "./actualStudy";

describe("实际学习填写校验", () => {
  it("可跳过，填写后必须有内容", () => {
    expect(actualStudyError([])).toBe("");
    expect(actualStudyError([{ subject: "math", scope: "ongoing", content: "  " }])).toContain(
      "填写"
    );
    expect(actualStudyError([{ subject: "math", scope: "today", content: "今天临时学线代" }])).toBe(
      ""
    );
  });
  it("同一科目不能提交相互冲突的方向", () => {
    expect(
      actualStudyError([
        { subject: "math", scope: "ongoing", content: "先高数" },
        { subject: "math", scope: "ongoing", content: "先线代" },
      ])
    ).toContain("合并");
  });
  it("结束方向允许不填写内容", () => {
    expect(actualStudyError([{ subject: "math", scope: "end", content: "" }])).toBe("");
  });
});
