import { describe, expect, it } from "vitest";
import { liquidGlassPulse } from "./liquidGlassMotion";

describe("navigation refraction settling", () => {
  it("returns to neutral before the carriage stops", () => {
    expect(liquidGlassPulse(0)).toBe(0);
    expect(liquidGlassPulse(0.18)).toBeCloseTo(0.26);
    expect(liquidGlassPulse(0.8)).toBe(0);
    expect(liquidGlassPulse(1)).toBe(0);
  });
  it("decays continuously without a layer-switch jump", () => {
    let previous = liquidGlassPulse(0.18);
    for (let t = 0.19; t <= 1; t += 0.01) {
      const current = liquidGlassPulse(t);
      expect(current).toBeGreaterThanOrEqual(0);
      expect(current).toBeLessThanOrEqual(previous);
      expect(previous - current).toBeLessThan(0.007);
      previous = current;
    }
  });
  it("keeps the current strength when a switch interrupts travel", () => {
    const current = liquidGlassPulse(0.35);
    expect(liquidGlassPulse(0, current)).toBe(current);
    expect(liquidGlassPulse(1, current)).toBe(0);
  });
});
