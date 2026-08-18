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
    path: "/week-plan",
    name: "week-plan",
    component: () => import("@/views/WeekPlanView.vue"),
    meta: { title: "周计划", icon: "CalendarDays" },
  },
  {
    path: "/history-plans",
    name: "history-plans",
    component: () => import("@/views/HistoryPlansView.vue"),
    meta: { title: "历史计划", icon: "History" },
  },
  {
    path: "/textbooks",
    name: "textbooks",
    component: () => import("@/views/TextbooksView.vue"),
    meta: { title: "教材", icon: "BookOpen" },
  },
  {
    path: "/review",
    name: "review",
    component: () => import("@/views/ReviewView.vue"),
    meta: { title: "复盘", icon: "ClipboardCheck" },
  },
  {
    path: "/doubt",
    name: "doubt",
    component: () => import("@/views/DoubtView.vue"),
    meta: { title: "解惑", icon: "HelpCircle" },
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
  {
    path: "/debug",
    name: "debug",
    component: () => import("@/views/DebugView.vue"),
    meta: { title: "调试", icon: "Bug" },
  },
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
      // 加载失败时放行，避免阻塞应用
      return true;
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
