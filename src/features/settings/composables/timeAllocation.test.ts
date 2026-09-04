/**
 * 学科学习时间占比 — 纯函数算法单测
 *
 * 覆盖：
 * 1. spreadToSum：largest-remainder 整数守恒、全 0 权重均分、小数并列按 key 顺序。
 * 2. deriveFromWeeklyHours：按周学时推导默认占比。
 * 3. normalizeAllocation：非活跃清零、缺失回填周学时、缩放到 100、全 0 回退。
 * 4. adjustAllocation：等比联动、clamp、单活跃科目、总和恒为 100。
 */
import { describe, expect, it } from "vitest";
import type { SubjectKey } from "@/types/state";
import type { SubjectTimeAllocation } from "@/types/settings";
import {
  ALLOCATION_KEYS,
  adjustAllocation,
  deriveFromWeeklyHours,
  emptyAllocation,
  normalizeAllocation,
  spreadToSum,
} from "./timeAllocation";

const allActive: Record<SubjectKey, boolean> = {
  math: true,
  english: true,
  politics: true,
  professional: true,
};

const zeroWeekly: Record<SubjectKey, number> = { math: 0, english: 0, politics: 0, professional: 0 };

function total(alloc: SubjectTimeAllocation): number {
  return ALLOCATION_KEYS.reduce((s, k) => s + (alloc[k] ?? 0), 0);
}

describe("spreadToSum", () => {
  it("按 largest-remainder 分配，总和恰为 target", () => {
    // raw = [7.5, 2.5] → floor [7, 2]，余 1 → 小数并列时按 key 顺序给 math
    const out = spreadToSum(["math", "english"], [3, 1], 10);
    expect(out).toEqual([8, 2]);
    expect(out.reduce((a, b) => a + b, 0)).toBe(10);
  });

  it("小数部分大的优先补余", () => {
    // raw = [3.33, 6.67] → floor [3, 6]，余 1 → 6.67 小数更大 → english +1
    const out = spreadToSum(["math", "english"], [1, 2], 10);
    expect(out).toEqual([3, 7]);
  });

  it("权重全 0 或 target 非正时均分", () => {
    expect(spreadToSum(["math", "english", "politics"], [0, 0, 0], 10)).toEqual([4, 3, 3]);
    expect(spreadToSum(["math", "english"], [5, 5], 0)).toEqual([0, 0]);
  });

  it("空 keys 返回空数组", () => {
    expect(spreadToSum([], [], 10)).toEqual([]);
  });
});

describe("deriveFromWeeklyHours", () => {
  it("按周学时占比推导", () => {
    const out = deriveFromWeeklyHours(
      { math: 6, english: 3, politics: 0, professional: 3 },
      allActive,
    );
    expect(out.math).toBe(50);
    expect(out.english).toBe(25);
    expect(out.politics).toBe(0);
    expect(out.professional).toBe(25);
  });

  it("周学时全 0 时活跃科目均分到 100", () => {
    const out = deriveFromWeeklyHours(zeroWeekly, allActive);
    expect(out).toEqual({ math: 25, english: 25, politics: 25, professional: 25 });
  });

  it("非活跃科目为 0；无活跃科目返回全 0", () => {
    const out = deriveFromWeeklyHours({ math: 10, english: 0, politics: 0, professional: 0 }, {
      math: true,
      english: false,
      politics: false,
      professional: false,
    });
    expect(out).toEqual({ math: 100, english: 0, politics: 0, professional: 0 });
    expect(deriveFromWeeklyHours(zeroWeekly, {
      math: false,
      english: false,
      politics: false,
      professional: false,
    })).toEqual(emptyAllocation());
  });
});

describe("normalizeAllocation", () => {
  it("存储占比缺省科目按周学时补入，合计缩放到 100", () => {
    const out = normalizeAllocation(
      { math: 60, english: 40 },
      { math: 10, english: 5, politics: 5, professional: 0 },
      allActive,
    );
    // 补入 politics=5 后 60:40:5 缩放到 100 → 57:38:5（largest-remainder 补余给 politics）
    expect(total(out)).toBe(100);
    expect(out.math).toBe(57);
    expect(out.english).toBe(38);
    expect(out.politics).toBe(5);
    expect(out.professional).toBe(0);
  });

  it("非活跃科目清零", () => {
    const out = normalizeAllocation(
      { math: 50, english: 30, politics: 20, professional: 0 },
      zeroWeekly,
      { math: true, english: true, politics: false, professional: true },
    );
    expect(out.politics).toBe(0);
    expect(out.math + out.english + out.professional).toBe(100);
  });

  it("活跃科目全 0 时回退周学时推导", () => {
    const out = normalizeAllocation({ math: 0, english: 0, politics: 0, professional: 0 }, {
      math: 6,
      english: 3,
      politics: 0,
      professional: 3,
    }, allActive);
    expect(out.math).toBe(50);
  });

  it("浮点占比缩放到 100 且无漂移", () => {
    const out = normalizeAllocation(
      { math: 33.3, english: 33.3, politics: 33.3 },
      zeroWeekly,
      { math: true, english: true, politics: true, professional: false },
    );
    expect(total(out)).toBe(100);
  });

  it("无活跃科目返回全 0", () => {
    expect(normalizeAllocation({}, zeroWeekly, {
      math: false,
      english: false,
      politics: false,
      professional: false,
    })).toEqual(emptyAllocation());
  });
});

describe("adjustAllocation", () => {
  it("把某科调整到新值，其余科目等比联动且合计恒为 100", () => {
    const current: SubjectTimeAllocation = { math: 40, english: 30, politics: 20, professional: 10 };
    const out = adjustAllocation(current, "math", 50, allActive);
    expect(out.math).toBe(50);
    expect(total(out)).toBe(100);
    // 其余按原比例 30:20:10 = 3:2:1 分配 50 → 25 / 17(16.67 进位) / 8(8.33 舍)
    expect(out.english).toBe(25);
    expect(out.politics).toBe(17);
    expect(out.professional).toBe(8);
  });

  it("超过 100 时 clamp 到 100，其余科目清零", () => {
    const current: SubjectTimeAllocation = { math: 40, english: 30, politics: 20, professional: 10 };
    const out = adjustAllocation(current, "math", 150, allActive);
    expect(out.math).toBe(100);
    expect(total(out)).toBe(100);
    expect(out.english + out.politics + out.professional).toBe(0);
  });

  it("负数 clamp 到 0，其余科目等比放大", () => {
    const current: SubjectTimeAllocation = { math: 40, english: 30, politics: 20, professional: 10 };
    const out = adjustAllocation(current, "math", -10, allActive);
    expect(out.math).toBe(0);
    expect(total(out)).toBe(100);
  });

  it("单活跃科目恒为 100", () => {
    const active: Record<SubjectKey, boolean> = {
      math: true,
      english: false,
      politics: false,
      professional: false,
    };
    const out = adjustAllocation({ math: 50, english: 50, politics: 0, professional: 0 }, "math", 30, active);
    expect(out).toEqual({ math: 100, english: 0, politics: 0, professional: 0 });
  });

  it("其他科目占比全 0 时均摊余额", () => {
    const current: SubjectTimeAllocation = { math: 100, english: 0, politics: 0, professional: 0 };
    const out = adjustAllocation(current, "math", 40, allActive);
    expect(out.math).toBe(40);
    // 余额 60 均摊给 3 科
    expect(out.english).toBe(20);
    expect(out.politics).toBe(20);
    expect(out.professional).toBe(20);
    expect(total(out)).toBe(100);
  });

  it("无活跃科目时原样返回", () => {
    const current: SubjectTimeAllocation = { math: 40, english: 30, politics: 20, professional: 10 };
    const active: Record<SubjectKey, boolean> = {
      math: false,
      english: false,
      politics: false,
      professional: false,
    };
    expect(adjustAllocation(current, "math", 50, active)).toEqual(current);
  });

  it("任意随机调整后总和恒为 100", () => {
    const current: SubjectTimeAllocation = { math: 40, english: 30, politics: 20, professional: 10 };
    for (const key of ALLOCATION_KEYS) {
      for (const v of [0, 1, 33, 67, 99, 100]) {
        const out = adjustAllocation(current, key, v, allActive);
        expect(total(out), `${key}=${v}`).toBe(100);
      }
    }
  });
});
