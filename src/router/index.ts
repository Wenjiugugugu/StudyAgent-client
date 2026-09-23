import { createRouter, createWebHistory } from "vue-router";
import type { RouteRecordRaw } from "vue-router";
import { useSettingsStore } from "@/stores/settings";

const routes: RouteRecordRaw[] = [
  {
    path: "/",
    redirect: "/dashboard",
  },
  {
    path: "/onboarding",
    name: "onboarding",
    component: () => import("@/views/OnboardingView.vue"),
    meta: { title: "初始配置", standalone: true },
  },
  {
    path: "/dashboard",
    name: "dashboard",
    component: () => import("@/views/DashboardView.vue"),
    meta: { title: "工作台", icon: "LayoutDashboard" },
  },
  {
    path: "/today",
    name: "plan",
    component: () => import("@/views/TodayView.vue"),
    meta: { title: "计划", icon: "Calendar" },
  },
  {
    path: "/goal-plan",
    name: "goal-plan",
    component: () => import("@/views/GoalPlanView.vue"),
    meta: { title: "目标计划", icon: "Target" },
  },
  {
    path: "/history-plans",
    name: "history-plans",
    component: () => import("@/views/HistoryPlansView.vue"),
    meta: { title: "历史计划", icon: "History" },
  },
  {
    path: "/progress",
    name: "progress",
    component: () => import("@/views/ProgressView.vue"),
    meta: { title: "进度", icon: "ClipboardList" },
  },
  {
    path: "/review",
    name: "review",
    component: () => import("@/views/ReviewView.vue"),
    meta: { title: "复盘", icon: "ClipboardCheck" },
  },
  {
    path: "/focus",
    name: "focus",
    component: () => import("@/views/FocusView.vue"),
    meta: { title: "专注", icon: "Timer" },
  },
  {
    path: "/timeline",
    name: "timeline",
    component: () => import("@/views/TimelineView.vue"),
    meta: { title: "时间线", icon: "GitBranch", reserved: true },
  },
  {
    path: "/analytics",
    name: "analytics",
    component: () => import("@/views/AnalyticsView.vue"),
    meta: { title: "分析", icon: "BarChart3" },
  },
  ...(import.meta.env.DEV || import.meta.env.VITE_ENABLE_DEBUG === "true"
    ? [
        {
          path: "/debug",
          name: "debug",
          component: () => import("@/views/DebugView.vue"),
          meta: { title: "调试", icon: "Bug" },
        } satisfies RouteRecordRaw,
      ]
    : []),
  {
    path: "/settings",
    name: "settings",
    component: () => import("@/views/SettingsView.vue"),
    meta: { title: "设置", icon: "Settings" },
  },
  // M36：404 兜底路由，避免访问未定义路径时显示空白页
  {
    path: "/:pathMatch(.*)*",
    redirect: "/dashboard",
  },
];

const router = createRouter({
  history: createWebHistory(),
  routes,
});

// 引导状态守卫：未完成引导时重定向到 /onboarding
router.beforeEach(async (to) => {
  // 引导页本身无需检查
  if (to.path === "/onboarding") {
    return true;
  }

  const settingsStore = useSettingsStore();
  // 首次进入时确保设置已加载
  if (!settingsStore.settings) {
    try {
      await settingsStore.load();
    } catch {
      // 不使用默认/Mock 设置伪装成功；引导用户进入可重试的设置错误页。
      return to.path === "/settings" ? true : { path: "/settings", query: { loadError: "1" } };
    }
  }

  if (!settingsStore.onboardingCompleted) {
    return { path: "/onboarding" };
  }

  return true;
});

router.afterEach((to) => {
  const title = (to.meta.title as string) || "StudyAgent";
  document.title = `${title} — StudyAgent`;
});

export default router;
