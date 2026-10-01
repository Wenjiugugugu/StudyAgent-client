import { createApp } from "vue";
import { createPinia } from "pinia";
import router from "./router";
import App from "./App.vue";

// Styles
import "katex/dist/katex.min.css";
import "./styles/variables.css";
import "./styles/global.css";
import "./styles/liquid-glass.css";

const app = createApp(App);

app.use(createPinia());
app.use(router);

app.mount("#app");

// Keep specular light local to the surface under the pointer.
(function initLiquidGlassMouseTracker() {
  if (typeof window === "undefined") return;

  let rafId: number | null = null;
  let pendingTarget: HTMLElement | null = null;
  let pendingX = 50;
  let pendingY = 0;

  const updateVars = () => {
    pendingTarget?.style.setProperty("--lg-x", `${pendingX.toFixed(1)}%`);
    pendingTarget?.style.setProperty("--lg-y", `${pendingY.toFixed(1)}%`);
    rafId = null;
  };

  document.addEventListener(
    "pointermove",
    (e) => {
      if (
        document.documentElement.dataset.visualMode !== "liquid-glass" ||
        window.matchMedia("(prefers-reduced-motion: reduce)").matches
      )
        return;
      const target = (e.target as Element).closest<HTMLElement>(
        ".hero-countdown, .focus-today, .focus-empty, .lg-preview, .ui-switch, .ui-button, .sidebar, .modal-dialog"
      );
      if (!target) return;
      const rect = target.getBoundingClientRect();
      pendingTarget = target;
      pendingX = ((e.clientX - rect.left) / rect.width) * 100;
      pendingY = ((e.clientY - rect.top) / rect.height) * 100;
      if (rafId === null) {
        rafId = requestAnimationFrame(updateVars);
      }
    },
    { passive: true }
  );
})();
