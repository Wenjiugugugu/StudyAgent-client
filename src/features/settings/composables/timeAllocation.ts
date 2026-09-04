/**
 * 学科学习时间占比 — 纯函数算法模块（无 Vue 依赖，可单测）
 *
 * 占比语义：百分比（0-100），活跃科目合计恒为 100。
 * 与后端 Rust 端 `weighted_spread`（largest-remainder）算法对称，
 * 保证整数守恒、无浮点漂移、无 0 除。
 */
import type { SubjectTimeAllocation } from "@/types/settings";
import type { SubjectKey } from "@/types/state";

/** 全部学科 key（固定顺序，用于确定性输出） */
export const ALLOCATION_KEYS: SubjectKey[] = ["math", "english", "politics", "professional"];

/** 滑块步长（百分比，整数） */
export const SLIDER_STEP = 1;

/** 空占比对象（全 0） */
export function emptyAllocation(): SubjectTimeAllocation {
  return { math: 0, english: 0, politics: 0, professional: 0 };
}

/**
 * largest-remainder 归一化：把 shares（相对权重，可非整数）分配到 keys，总和恰为 target。
 * 与后端 weighted_spread 同一算法：floor 后按小数部分降序补余，小数相同时按 key 顺序。
 */
export function spreadToSum(keys: SubjectKey[], shares: number[], target: number): number[] {
  if (keys.length === 0) return [];
  const total = shares.reduce((s, v) => s + v, 0);
  if (!(total > 0) || target <= 0) {
    // 权重全 0 或目标非正：均分（largest-remainder），保证总和恰为 target
    const base = Math.floor(target / keys.length);
    const rem = target - base * keys.length;
    return keys.map((_, i) => base + (i < rem ? 1 : 0));
  }
  const raw = shares.map((v) => (v / total) * target);
  const result = raw.map((v) => Math.floor(v));
  let remaining = target - result.reduce((s, v) => s + v, 0);
  if (remaining > 0) {
    const order = raw
      .map((v, i) => ({ v, i }))
      .sort((a, b) => {
        const diff = b.v - Math.floor(b.v) - (a.v - Math.floor(a.v));
        return diff !== 0 ? diff : a.i - b.i;
      })
      .map((x) => x.i);
    for (let idx = 0; remaining > 0; idx++, remaining--) {
      result[order[idx % order.length]] += 1;
    }
  }
  return result;
}

/**
 * 从各科周学时推导默认占比。
 * 活跃科目占比 = weekly / sum×100；sum ≤ 0 时活跃科目均分；非活跃科目为 0。
 */
export function deriveFromWeeklyHours(
  weekly: Record<SubjectKey, number>,
  active: Record<SubjectKey, boolean>,
): SubjectTimeAllocation {
  const out = emptyAllocation();
  const activeKeys = ALLOCATION_KEYS.filter((k) => active[k]);
  if (activeKeys.length === 0) return out;
  const sum = activeKeys.reduce((s, k) => s + Math.max(0, weekly[k] ?? 0), 0);
  if (sum > 0) {
    for (const k of activeKeys) {
      out[k] = Math.round(((Math.max(0, weekly[k] ?? 0) / sum) * 1000) / 10);
    }
  } else {
    const base = Math.floor(100 / activeKeys.length);
    const rem = 100 - base * activeKeys.length;
    activeKeys.forEach((k, i) => {
      out[k] = base + (i < rem ? 1 : 0);
    });
  }
  return out;
}

/**
 * 归一化存储的占比：
 * - 非活跃科目清零；
 * - 活跃但缺失的科目按周学时补入；
 * - 活跃科目合计缩放到 100（largest-remainder 修正浮点漂移）；
 * - 活跃科目全为 0 时回退 deriveFromWeeklyHours。
 */
export function normalizeAllocation(
  stored: Partial<SubjectTimeAllocation>,
  weekly: Record<SubjectKey, number>,
  active: Record<SubjectKey, boolean>,
): SubjectTimeAllocation {
  const activeKeys = ALLOCATION_KEYS.filter((k) => active[k]);
  if (activeKeys.length === 0) return emptyAllocation();

  const raw: Partial<SubjectTimeAllocation> = {};
  for (const k of ALLOCATION_KEYS) {
    if (!active[k]) {
      raw[k] = 0;
    } else {
      const v = stored?.[k];
      raw[k] = typeof v === "number" && isFinite(v) && v > 0 ? v : Math.max(0, weekly[k] ?? 0);
    }
  }
  const sum = activeKeys.reduce((s, k) => s + (raw[k] ?? 0), 0);
  if (!(sum > 0)) {
    return deriveFromWeeklyHours(weekly, active);
  }
  const scaled = activeKeys.map((k) => ((raw[k] ?? 0) / sum) * 100);
  const spread = spreadToSum(activeKeys, scaled, 100);
  const out = emptyAllocation();
  activeKeys.forEach((k, i) => {
    out[k] = spread[i];
  });
  return out;
}

/**
 * 等比联动：把 key 学科调整到 newValue（clamp 0-100）。
 * 其余活跃科目按各自占比占其余额的比例缩放（largest-remainder 保证总和恰为 100）；
 * 其余额（otherSum）为 0 时均摊。返回新的完整分配。
 */
export function adjustAllocation(
  current: SubjectTimeAllocation,
  key: SubjectKey,
  newValue: number,
  active: Record<SubjectKey, boolean>,
): SubjectTimeAllocation {
  const activeKeys = ALLOCATION_KEYS.filter((k) => active[k]);
  if (activeKeys.length === 0) return { ...current };
  if (activeKeys.length === 1) {
    const out = emptyAllocation();
    out[activeKeys[0]] = 100;
    return out;
  }
  const clamped = Math.max(0, Math.min(100, Math.round(newValue)));
  const others = activeKeys.filter((k) => k !== key);
  const need = 100 - clamped; // 其余科目需分配的总和（整数）
  const out = { ...current };
  out[key] = clamped;

  const otherSum = others.reduce((s, k) => s + Math.max(0, current[k] ?? 0), 0);
  if (otherSum <= 0) {
    // 防 0 除：其他占比全为 0 → 均摊 need
    const base = Math.floor(need / others.length);
    const rem = need - base * others.length;
    others.forEach((k, i) => {
      out[k] = base + (i < rem ? 1 : 0);
    });
  } else {
    const rawShares = others.map((k) => ((Math.max(0, current[k] ?? 0) / otherSum) * need));
    const spread = spreadToSum(others, rawShares, need);
    others.forEach((k, i) => {
      out[k] = spread[i];
    });
  }
  return out;
}
