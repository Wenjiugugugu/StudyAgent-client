import type { ObjectDirective } from "vue";
import { createLiquidGlassMap } from "@/utils/liquidGlass";
import { liquidGlassPulse } from "@/utils/liquidGlassMotion";

const ns = "http://www.w3.org/2000/svg";
type LensOptions = { strength?: number };
const lenses = new Map<
  HTMLElement,
  {
    filter: SVGFilterElement;
    key: string;
    strength: number;
    pulseFrame?: number;
    pulseStrength?: number;
  }
>();
let root: SVGSVGElement | undefined;
let resize: ResizeObserver | undefined;
let mode: MutationObserver | undefined;
let frame = 0;
let serial = 0;

function svg<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string>) {
  const element = document.createElementNS(ns, tag);
  for (const [name, value] of Object.entries(attrs)) element.setAttribute(name, value);
  return element;
}

function refresh() {
  frame = 0;
  const enabled = document.documentElement.dataset.visualMode === "liquid-glass";
  for (const [element, lens] of lenses) {
    if (!enabled) {
      cancelAnimationFrame(lens.pulseFrame ?? 0);
      lens.pulseStrength = undefined;
      element.style.removeProperty("--lg-refraction");
      lens.filter.replaceChildren();
      lens.key = "";
      continue;
    }
    const width = element.offsetWidth;
    const height = element.offsetHeight;
    if (!width || !height) continue;
    const radius = Math.min(
      parseFloat(getComputedStyle(element).borderTopLeftRadius) || 0,
      width / 2,
      height / 2
    );
    const key = `${width}:${height}:${radius}`;
    if (key === lens.key) continue;
    const map = createLiquidGlassMap(width, height, radius);
    if (!map) continue;
    lens.key = key;
    const scale = Math.min(28, height * (lens.pulseStrength ?? lens.strength));
    // Leave room to sample beyond the silhouette while a small thumb swells.
    // Clipping the filter to the lens box creates sharp transparent cutouts.
    const padding = Math.ceil(Math.min(28, height * 0.42) / 2) + 2;
    lens.filter.setAttribute("x", String(-padding));
    lens.filter.setAttribute("y", String(-padding));
    lens.filter.setAttribute("width", String(width + padding * 2));
    lens.filter.setAttribute("height", String(height + padding * 2));
    lens.filter.replaceChildren(
      svg("feImage", {
        href: map,
        x: "0",
        y: "0",
        width: String(width),
        height: String(height),
        preserveAspectRatio: "none",
        result: "rim",
      }),
      svg("feDisplacementMap", {
        in: "SourceGraphic",
        in2: "rim",
        scale: String(scale),
        xChannelSelector: "R",
        yChannelSelector: "G",
      })
    );
    element.style.setProperty("--lg-refraction", `url("#${lens.filter.id}")`);
  }
}

function schedule() {
  if (!frame) frame = requestAnimationFrame(refresh);
}

/** Hold a lens while dragging without rebuilding its geometry. */
export function holdLiquidGlass(element: HTMLElement | null, strength = 0.22) {
  const lens = element && lenses.get(element);
  if (!element || !lens) return;
  cancelAnimationFrame(lens.pulseFrame ?? 0);
  lens.pulseFrame = undefined;
  lens.pulseStrength = matchMedia("(prefers-reduced-motion: reduce)").matches
    ? lens.strength
    : strength;
  lens.filter
    .querySelector("feDisplacementMap")
    ?.setAttribute("scale", String(Math.min(28, element.offsetHeight * lens.pulseStrength)));
}

/** Animate displacement only; keep the sampling region and stacking order stable. */
export function pulseLiquidGlass(element: HTMLElement | null, duration = 480) {
  const lens = element && lenses.get(element);
  if (!element || !lens) return;
  cancelAnimationFrame(lens.pulseFrame ?? 0);
  if (
    document.documentElement.dataset.visualMode !== "liquid-glass" ||
    matchMedia("(prefers-reduced-motion: reduce)").matches
  )
    return;
  const started = performance.now();
  const initial = lens.pulseStrength ?? lens.strength;
  const tick = (now: number) => {
    const t = Math.min(1, (now - started) / duration);
    lens.pulseStrength = liquidGlassPulse(t, initial, lens.strength);
    lens.filter
      .querySelector("feDisplacementMap")
      ?.setAttribute("scale", String(Math.min(28, element.offsetHeight * lens.pulseStrength)));
    if (t < 1) lens.pulseFrame = requestAnimationFrame(tick);
    else {
      lens.pulseFrame = undefined;
      lens.pulseStrength = undefined;
    }
  };
  lens.pulseFrame = requestAnimationFrame(tick);
}

/** Geometry is refreshed on resize, never on pointer movement or animation frames. */
export const vLiquidGlass: ObjectDirective<HTMLElement, LensOptions | undefined> = {
  mounted(element, binding) {
    if (!root) {
      root = svg("svg", { width: "0", height: "0", "aria-hidden": "true", focusable: "false" });
      root.style.cssText = "position:fixed;pointer-events:none;overflow:hidden";
      document.body.append(root);
      resize = new ResizeObserver(schedule);
      mode = new MutationObserver(schedule);
      mode.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ["data-visual-mode"],
      });
    }
    const filter = svg("filter", {
      id: `lg-lens-${++serial}`,
      x: "0",
      y: "0",
      width: "100%",
      height: "100%",
      filterUnits: "userSpaceOnUse",
      primitiveUnits: "userSpaceOnUse",
      // linearRGB shifts the encoded neutral value, distorting the centre.
      "color-interpolation-filters": "sRGB",
    });
    root.append(filter);
    lenses.set(element, { filter, key: "", strength: binding.value?.strength ?? 0.42 });
    resize?.observe(element);
    schedule();
  },
  updated: schedule,
  unmounted(element) {
    cancelAnimationFrame(lenses.get(element)?.pulseFrame ?? 0);
    resize?.unobserve(element);
    lenses.get(element)?.filter.remove();
    lenses.delete(element);
    element.style.removeProperty("--lg-refraction");
    if (!lenses.size) {
      resize?.disconnect();
      mode?.disconnect();
      root?.remove();
      root = undefined;
      cancelAnimationFrame(frame);
      frame = 0;
    }
  },
};
