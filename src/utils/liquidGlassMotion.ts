/** Scale the lens separately from translation; rapid switches retain the current shape. */
export function swellLiquidGlass(
  element: HTMLElement | null,
  previous?: Animation,
  swell: { x: number; y: number; duration: number } = { x: 1.045, y: 1.24, duration: 620 }
): Animation | undefined {
  if (!element) return;
  const transform = getComputedStyle(element).transform;
  previous?.cancel();
  if (
    document.documentElement.dataset.visualMode !== "liquid-glass" ||
    matchMedia("(prefers-reduced-motion: reduce)").matches
  )
    return;
  return element.animate(
    [
      { transform: transform === "none" ? "scale(1)" : transform, offset: 0 },
      { transform: `scale(${swell.x}, ${swell.y})`, offset: 0.28 },
      { transform: "scale(0.995, 0.97)", offset: 0.72 },
      { transform: "scale(1)", offset: 1 },
    ],
    { duration: swell.duration, easing: "cubic-bezier(0.22, 0.8, 0.3, 1)" }
  );
}
/** Smooth refraction returns to neutral before the carriage finishes decelerating. */
export function liquidGlassPulse(t: number, initial = 0, resting = 0): number {
  const smooth = (v: number) => {
    const x = Math.max(0, Math.min(1, v));
    return x * x * (3 - 2 * x);
  };
  const peak = Math.max(initial, 0.26);
  if (t < 0.18) return initial + (peak - initial) * smooth(t / 0.18);
  return peak + (resting - peak) * smooth((t - 0.18) / 0.62);
}
