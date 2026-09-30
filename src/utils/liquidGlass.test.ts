import { describe, expect, it } from "vitest";
import { liquidGlassVector } from "./liquidGlass";

describe("liquid glass lens geometry", () => {
  it("leaves the centre and points outside the rounded silhouette undistorted", () => {
    expect(liquidGlassVector(100, 20, 200, 40, 20)).toEqual([0, 0]);
    expect(liquidGlassVector(50, 20, 200, 40, 20)).toEqual([0, 0]);
    expect(liquidGlassVector(1, 1, 200, 40, 20)).toEqual([0, 0]);
  });

  it("bends opposite rims symmetrically inward", () => {
    const left = liquidGlassVector(4, 20, 200, 40, 20);
    const right = liquidGlassVector(196, 20, 200, 40, 20);
    expect(left[0]).toBeGreaterThan(0.8);
    expect(right[0]).toBeCloseTo(-left[0]);
    expect(Math.abs(left[1])).toBe(0);
    const top = liquidGlassVector(100, 4, 200, 40, 20);
    const bottom = liquidGlassVector(100, 36, 200, 40, 20);
    expect(top[1]).toBeGreaterThan(0.8);
    expect(bottom[1]).toBeCloseTo(-top[1]);
  });

  it("keeps the bevel thickness when a capsule becomes wider", () => {
    expect(liquidGlassVector(4, 20, 80, 40, 20)).toEqual(liquidGlassVector(4, 20, 320, 40, 20));
    expect(liquidGlassVector(40, 4, 80, 40, 20)).toEqual(liquidGlassVector(160, 4, 320, 40, 20));
  });

  it("keeps corner displacement finite and within the texture encoding range", () => {
    for (const [width, height, radius] of [
      [48, 40, 999],
      [200, 40, 999],
      [330, 152, 76],
      [220, 700, 28],
    ]) {
      for (let y = 0; y < height; y += 3) {
        for (let x = 0; x < width; x += 3) {
          const [dx, dy] = liquidGlassVector(x, y, width, height, radius);
          expect(Number.isFinite(dx) && Number.isFinite(dy)).toBe(true);
          expect(Math.hypot(dx, dy)).toBeLessThanOrEqual(1.000001);
        }
      }
    }
  });
});
