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

// Liquid Glass: track mouse position for dynamic edge highlights
(function initLiquidGlassMouseTracker() {
  if (typeof window === "undefined") return;

  let rafId: number | null = null;
  let pendingX = 0.5;
  let pendingY = 0.5;

  const updateVars = () => {
    document.documentElement.style.setProperty("--lg-mouse-x", pendingX.toFixed(4));
    document.documentElement.style.setProperty("--lg-mouse-y", pendingY.toFixed(4));
    rafId = null;
  };

  document.addEventListener(
    "mousemove",
    (e) => {
      pendingX = e.clientX / window.innerWidth;
      pendingY = e.clientY / window.innerHeight;
      if (rafId === null) {
        rafId = requestAnimationFrame(updateVars);
      }
    },
    { passive: true }
  );
})();
