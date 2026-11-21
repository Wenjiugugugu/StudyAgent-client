import * as api from "@/api";
import { emit } from "@tauri-apps/api/event";
import { nextTick } from "vue";
import { todayString } from "@/utils/date";

export const desktopSmokeMode = Boolean(
  (window as Window & { __STUDYAGENT_DESKTOP_SMOKE__?: boolean }).__STUDYAGENT_DESKTOP_SMOKE__
);
export async function runDesktopSmoke() {
  try {
    await nextTick();
    const state = await api.getState();
    const plan = await api.getPlanByDate(todayString());
    await api.setUiFlag("desktop_smoke", "written-through-ipc");
    const flag = await api.getUiFlag("desktop_smoke");
    await emit("studyagent-desktop-smoke", {
      mounted: document.querySelector("[data-desktop-smoke]") !== null,
      state: state.progress.total_study_days === 42,
      plan: plan.meta.date === todayString(),
      write_read: flag === "written-through-ipc",
    });
  } catch (error) {
    await emit("studyagent-desktop-smoke", { error: String(error) });
  }
}
