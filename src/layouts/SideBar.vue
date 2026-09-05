<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted } from "vue";
import { useRoute } from "vue-router";
import { useTheme } from "@/composables/useTheme";
import { useSettingsStore } from "@/stores/settings";
import { getUiFlag, setUiFlag } from "@/api";
import Logo from "@/components/Logo.vue";
import {
  LayoutDashboard,
  Calendar,
  CalendarDays,
  History,
  BookOpen,
  ClipboardCheck,
  GitBranch,
  BarChart3,
  Settings,
  Moon,
  SunMedium,
  Bug,
  HelpCircle,
  Timer,
  ClipboardList,
  ChevronsLeft,
  ChevronsRight,
  Target,
} from "lucide-vue-next";
import { useAppVersion } from "@/version";

const { theme, toggleTheme } = useTheme();
const settingsStore = useSettingsStore();
const route = useRoute();
const isDev = import.meta.env.DEV;

// ── 侧边栏收展（收起后仅显示图标）──
// M7：持久化改走「后端 ui_flags 文件」（localStorage 部分环境随重启丢失）；
// 本地值先用于首帧渲染，挂载后以后端为准校正。
const COLLAPSE_KEY = "studyagent.sidebar.collapsed";
const collapsed = ref(localStorage.getItem(COLLAPSE_KEY) === "1");

onMounted(async () => {
  try {
    const stored = await getUiFlag(COLLAPSE_KEY);
    if (stored === "1" || stored === "0") collapsed.value = stored === "1";
  } catch {
    // 浏览器/命令失败：保持 localStorage 初值
  }
});

function toggleCollapse() {
  collapsed.value = !collapsed.value;
  localStorage.setItem(COLLAPSE_KEY, collapsed.value ? "1" : "0");
  void setUiFlag(COLLAPSE_KEY, collapsed.value ? "1" : "0").catch(() => {});
  nextTick(updateIndicator);
}

const navRef = ref<HTMLElement | null>(null);
const indicatorStyle = ref<{ transform: string; height: string; opacity: number }>({
  transform: "translateY(0px)",
  height: "0px",
  opacity: 0,
});

function updateIndicator() {
  requestAnimationFrame(() => {
    if (!navRef.value) return;
    // 「计划」页面固定跟随一级按钮，命中的二级子项仅文字高亮，
    // 避免在自动展开或切换子项时先命中二级链接导致指示条跳位。
    // 展开态「计划」一级按钮是 .nav-group 的直接子按钮。这里直接查询而非用模板 ref：
    // 该 ref 处于外层 v-for 模板作用域内，会被 Vue 收集成数组而非元素，
    // 导致读取 offsetTop/offsetHeight 失效、指示条无法跟随。
    const planItem = navRef.value.querySelector<HTMLElement>(".nav-group > button.nav-item");
    const active = (!collapsed.value && isPlanActive() ? planItem : null)
      ?? navRef.value.querySelector(".nav-item.active") as HTMLElement | null;
    if (!active) {
      indicatorStyle.value.opacity = 0;
      return;
    }
    indicatorStyle.value = {
      transform: `translateY(${active.offsetTop}px)`,
      height: `${active.offsetHeight}px`,
      opacity: 1,
    };
  });
}

/** 计划组「分裂/融入」动画结束后重算指示条，避免布局变化后位置偏移 */
function onPlanMorphDone() {
  updateIndicator();
}

interface NavItem {
  name: string;
  label: string;
  icon: any;
  path: string;
  reserved?: boolean;
}

/** 「计划」二级菜单：一级项 + 子项 */
const planGroup = {
  name: "plan",
  label: "计划",
  path: "/today",
  children: [
    { name: "today", label: "今日计划", icon: Calendar, path: "/today" },
    { name: "week-plan", label: "周计划", icon: CalendarDays, path: "/week-plan" },
    { name: "goal-plan", label: "目标计划", icon: Target, path: "/goal-plan" },
    { name: "history-plans", label: "历史计划", icon: History, path: "/history-plans" },
  ] as NavItem[],
};

type MenuEntry = { kind: "item"; item: NavItem } | { kind: "plan" };

/** 侧边栏菜单顺序：工作台 → 计划 → 专注 → 复盘 → 分析 → 进度 → 教材 → 解惑 → 时间线 */
const menuEntries: MenuEntry[] = [
  { kind: "item", item: { name: "dashboard", label: "工作台", icon: LayoutDashboard, path: "/dashboard" } },
  { kind: "plan" },
  { kind: "item", item: { name: "focus", label: "专注", icon: Timer, path: "/focus" } },
  { kind: "item", item: { name: "review", label: "复盘", icon: ClipboardCheck, path: "/review" } },
  { kind: "item", item: { name: "analytics", label: "分析", icon: BarChart3, path: "/analytics" } },
  { kind: "item", item: { name: "progress", label: "进度", icon: ClipboardList, path: "/progress" } },
  { kind: "item", item: { name: "textbooks", label: "教材", icon: BookOpen, path: "/textbooks" } },
  { kind: "item", item: { name: "doubt", label: "解惑", icon: HelpCircle, path: "/doubt" } },
  ...(isDev
    ? [{ kind: "item", item: { name: "timeline", label: "时间线", icon: GitBranch, path: "/timeline", reserved: true } } as MenuEntry]
    : []),
];

/** 「计划」二级菜单是否展开：命中任一子项或一级项时展开 */
const planOpen = ref(false);
/** 是否启用悬浮岛式侧边栏（来自设置） */
const isFloating = computed(() => settingsStore.sidebarStyle === "floating");
watch(
  () => route.path,
  (p) => {
    planOpen.value = planGroup.children.some((c) => c.path === p) || p === planGroup.path;
  },
  { immediate: true },
);

watch(
  [() => route.path, () => collapsed.value, () => planOpen.value],
  () => nextTick(updateIndicator),
  { immediate: true },
);

/** 是否为「计划」相关路由（用于一级项高亮与 indicator） */
function isPlanActive(): boolean {
  return planGroup.children.some((c) => c.path === route.path) || route.path === planGroup.path;
}

// 当前版本号（统一经 useAppVersion 读取，勿在此写死）
const { version } = useAppVersion();

// 调试入口可见性：开发模式，或版本号含 indev（如 0.5.7-indev）时对用户可见
const isDebugAvailable = computed(
  () => isDev || version.value.toLowerCase().includes("indev"),
);

/** 「计划」一级项点击：切换二级菜单展开/收起（保持原版行为） */
function onPlanClick() {
  planOpen.value = !planOpen.value;
}
</script>

<template>
  <aside class="sidebar" :class="{ collapsed, floating: isFloating }">
    <!-- App Brand / Drag Region -->
    <div class="brand" data-tauri-drag-region>
      <div v-if="settingsStore.showLogo" class="brand-icon">
        <Logo />
      </div>
      <div class="brand-text" :class="{ 'no-logo': !settingsStore.showLogo }">
        <span class="brand-name">StudyAgent</span>
        <span class="brand-tagline">考研学习智能体</span>
      </div>
    </div>

    <!-- Navigation -->
    <nav ref="navRef" class="nav">
      <div class="nav-indicator" :style="indicatorStyle" aria-hidden="true" />
      <template v-for="entry in menuEntries" :key="entry.kind === 'item' ? entry.item.name : 'plan'">
        <!-- 普通导航项 -->
        <router-link
          v-if="entry.kind === 'item'"
          :to="entry.item.path"
          class="nav-item"
          :class="{ reserved: entry.item.reserved }"
          active-class="active"
          :title="collapsed ? entry.item.label : ''"
        >
          <component :is="entry.item.icon" :size="19" :stroke-width="1.5" class="nav-icon" />
          <span class="nav-label">{{ entry.item.label }}</span>
          <span v-if="entry.item.name === 'doubt'" class="nav-badge">测试版</span>
        </router-link>

        <!-- 「计划」二级菜单 -->
        <div v-else class="nav-group">
          <!-- 收起态：一级「计划」图标分裂为 3 个二级菜单图标 -->
          <transition name="plan-cols" @after-enter="onPlanMorphDone" @after-leave="onPlanMorphDone">
            <div v-if="collapsed" class="plan-cols">
              <router-link
                v-for="c in planGroup.children"
                :key="c.name"
                :to="c.path"
                class="nav-item"
                active-class="active"
                :title="c.label"
              >
                <component :is="c.icon" :size="19" :stroke-width="1.5" class="nav-icon" />
                <span class="nav-label">{{ c.label }}</span>
              </router-link>
            </div>
          </transition>

          <!-- 展开态：一级菜单 + 内联二级菜单（二级图标融入回去） -->
          <template v-if="!collapsed">
            <button
              type="button"
              class="nav-item"
              :class="{ active: isPlanActive() }"
              :aria-expanded="planOpen"
              aria-controls="plan-subnav"
              @click="onPlanClick"
            >
              <component :is="Calendar" :size="19" :stroke-width="1.5" class="nav-icon" />
              <span class="nav-label">{{ planGroup.label }}</span>
              <span
                class="nav-chevron"
                :class="{ open: planOpen }"
                aria-hidden="true"
              ></span>
            </button>
            <div id="plan-subnav" v-show="planOpen" class="nav-children">
              <router-link
                v-for="c in planGroup.children"
                :key="c.name"
                :to="c.path"
                class="nav-item nav-child"
                active-class="active"
              >
                <component :is="c.icon" :size="17" :stroke-width="1.5" class="nav-icon child-icon" />
                <span class="nav-label">{{ c.label }}</span>
              </router-link>
            </div>
          </template>
        </div>
      </template>
    </nav>

    <!-- Bottom Section -->
    <div class="sidebar-bottom">
      <router-link
        v-if="isDebugAvailable"
        to="/debug"
        class="nav-item bottom-item"
        active-class="active"
        :title="collapsed ? '调试' : ''"
      >
        <Bug :size="19" :stroke-width="1.5" class="nav-icon" />
        <span class="nav-label">调试</span>
      </router-link>

      <router-link
        to="/settings"
        class="nav-item bottom-item"
        active-class="active"
        :title="collapsed ? '设置' : ''"
      >
        <Settings :size="19" :stroke-width="1.5" class="nav-icon" />
        <span class="nav-label">设置</span>
      </router-link>

      <button type="button" class="nav-item theme-toggle" @click="toggleTheme" :aria-label="theme === 'dark' ? '切换到浅色主题' : '切换到深色主题'" :title="collapsed ? (theme === 'dark' ? '浅色' : '深色') : ''">
        <component :is="theme === 'dark' ? SunMedium : Moon" :size="19" :stroke-width="1.5" class="nav-icon" />
        <span class="nav-label">{{ theme === "dark" ? "浅色" : "深色" }}</span>
      </button>

      <router-link
        to="/settings#settings-update"
        class="version-label"
        title="前往设置页检查更新"
      >
        <span>Beta {{ version }}</span>
      </router-link>

      <button type="button" class="nav-item collapse-toggle" @click="toggleCollapse" :aria-label="collapsed ? '展开侧边栏' : '收起侧边栏'" :title="collapsed ? '展开侧边栏' : '收起侧边栏'">
        <component :is="collapsed ? ChevronsRight : ChevronsLeft" :size="19" :stroke-width="1.5" class="nav-icon" />
        <span class="nav-label">{{ collapsed ? "" : "收起侧边栏" }}</span>
      </button>
    </div>
  </aside>
</template>

<style scoped>
/* Apple design library: frosted glass sidebar — the signature macOS material.
   Deeper translucent base + heavy backdrop blur reads clearly against the
   lighter content area; faint inner highlight simulates glass edge. */
.sidebar {
  box-sizing: border-box;
  flex: 0 0 var(--sidebar-width);
  width: var(--sidebar-width);
  min-width: var(--sidebar-width);
  height: 100%;
  background: var(--sidebar-bg);
  backdrop-filter: saturate(200%) blur(30px);
  -webkit-backdrop-filter: saturate(200%) blur(30px);
  display: flex;
  flex-direction: column;
  padding: var(--space-3) var(--space-2) var(--space-3);
  /* Reserve the same edge box in standard and liquid-glass modes. */
  border: var(--sidebar-control-border-width) solid transparent;
  border-right-color: var(--border-color);
  box-shadow: inset -1px 0 0 rgba(255, 255, 255, 0.4);
  user-select: none;
  /* 展开/收起时裁切仍在滑动的文字，避免溢出到内容区 */
  overflow: hidden;
  /* 宽度过渡（Apple motion curve）：flex 布局会逐帧重算，
     右侧内容随之平滑让位，无需对内容区单独加动画 */
  transition: flex-basis var(--transition-slow),
    width var(--transition-slow),
    min-width var(--transition-slow);
}

/* 收起态：仅显示图标 */
.sidebar.collapsed {
  flex-basis: var(--sidebar-collapsed-width);
  width: var(--sidebar-collapsed-width);
  min-width: var(--sidebar-collapsed-width);
}
/* 收起时「计划」内联二级菜单不再展示（由 plan-cols 分裂图标替代） */
.sidebar.collapsed .nav-children {
  display: none;
}

/* ── 展开/收起文字动画 ──
   展开：宽度先展开（Apple motion curve），文字随后淡入并自右滑入；
   收起：文字先淡出、滑向右缘，宽度再收窄，避免文字被挤压硬切。
   图标本身固定不动，与 Finder 侧边栏一致。 */
.brand-text,
.nav-label,
.nav-badge {
  opacity: 1;
  transform: translateX(0);
  transition: opacity 0.2s cubic-bezier(0.32, 0.72, 0, 1) 0.12s,
    transform 0.22s cubic-bezier(0.32, 0.72, 0, 1) 0.12s;
}
.sidebar.collapsed .brand-text,
.sidebar.collapsed .nav-label,
.sidebar.collapsed .nav-badge {
  opacity: 0;
  transform: translateX(12px);
  transition: opacity 0.1s ease, transform 0.1s ease;
}
/* 版本行收起时同步收起自身高度，避免底部留空 */
.sidebar.collapsed .version-label {
  opacity: 0;
  transform: translateX(12px);
  max-height: 0;
  padding-top: 0;
  padding-bottom: 0;
  transition: opacity 0.1s ease, transform 0.1s ease,
    max-height var(--transition-slow), padding var(--transition-slow);
}
/* 折叠箭头仅淡出（保留其自身的旋转过渡） */
.sidebar.collapsed .nav-chevron {
  opacity: 0;
}

/* 悬浮岛式侧边栏：四边留白 + 苹果规范大圆角 + 阴影，像一块悬浮在背景上的圆角面板 */
.sidebar.floating {
  flex: 0 0 calc(var(--sidebar-width) - 2 * var(--sidebar-floating-margin, var(--space-3)));
  margin: var(--sidebar-floating-margin, var(--space-3));
  width: auto;
  min-width: 0;
  height: calc(100% - 2 * var(--sidebar-floating-margin, var(--space-3)));
  border-radius: var(--radius-lg);
  border: 1px solid var(--border-color);
  border-right-color: var(--border-color);
  overflow: hidden;
  box-shadow: var(--shadow-xl), inset -1px 0 0 rgba(255, 255, 255, 0.4);
}
/* 收起态仍是悬浮岛：保留四边留白、圆角与阴影。
   content 宽度保持标准收起宽度（图标+内边距约需 59px），
   避免按边距减宽后裁切图标；边距由 margin 单独提供。 */
.sidebar.floating.collapsed {
  flex: 0 0 var(--sidebar-collapsed-width);
}
/* 悬浮岛模式下指示条较默认略收窄（留出呼吸空间），但仍保持较宽的观感 */
.sidebar.floating .nav-indicator {
  left: 2px;
  right: 2px;
}

/* Brand */
.brand {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  height: var(--header-height);
  min-height: var(--header-height);
  padding: 0 var(--space-2);
  margin-bottom: var(--space-2);
  flex-shrink: 0;
}

.brand-icon {
  width: 30px;
  height: 30px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.brand-text {
  display: flex;
  flex-direction: column;
  line-height: 1.25;
  min-width: 0;
  /* 收起动画滑动时裁切，避免品牌文字溢出侧边栏 */
  overflow: hidden;
}

.brand-text.no-logo {
  margin-left: 0;
}

.brand-name {
  font-size: var(--text-base);
  font-weight: var(--font-bold);
  color: var(--text-primary);
  letter-spacing: -0.02em;
}

.brand-tagline {
  font-size: 10px;
  color: var(--text-tertiary);
  font-weight: var(--font-medium);
}

.nav {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  position: relative;
}

.nav-indicator {
  position: absolute;
  left: 0;
  right: 0;
  top: 0;
  border-radius: var(--radius-md);
  background: var(--accent-subtle);
  z-index: 0;
  transition: transform 0.25s cubic-bezier(0.32, 0.72, 0, 1), opacity 0.2s ease;
  pointer-events: none;
}

.nav::-webkit-scrollbar {
  width: 0;
  height: 0;
}

/* Apple-style nav items: pill-shaped, restrained */
.nav-item {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  min-height: var(--sidebar-control-height);
  padding: var(--space-2) var(--space-3);
  border: var(--sidebar-control-border-width) solid transparent;
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: color var(--transition-fast);
  text-align: left;
  width: 100%;
  position: relative;
  text-decoration: none;
  letter-spacing: -0.01em;
  z-index: 1;
}

.nav-item:hover {
  background: var(--sidebar-item-hover);
  color: var(--text-primary);
}

.nav-item.active {
  background: transparent;
  color: var(--accent);
  font-weight: var(--font-semibold);
}

.nav-item.active .nav-icon {
  color: var(--accent);
}

.nav-item.reserved {
  opacity: 0.45;
}

.nav-item.reserved:hover {
  opacity: 0.75;
}

.nav-icon {
  flex-shrink: 0;
  color: currentColor;
  opacity: 0.8;
  transition: opacity var(--transition-fast);
}

.nav-item:hover .nav-icon {
  opacity: 1;
}

.nav-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 「测试版」徽标：靠右，小号胶囊，弱化强调 */
.nav-badge {
  margin-left: auto;
  flex-shrink: 0;
  font-size: 10px;
  line-height: 1;
  font-weight: var(--font-medium);
  color: var(--accent);
  background: var(--accent-subtle);
  border: 1px solid var(--accent-soft, var(--border-color));
  border-radius: 999px;
  padding: 2px 6px;
  letter-spacing: 0.02em;
}

/* 「计划」二级菜单 */
.nav-group {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
/* 收起态：一级图标分裂为 3 个二级图标的过渡 */
.plan-cols {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.plan-cols-enter-active {
  transition: opacity 0.22s ease, transform 0.22s cubic-bezier(0.32, 0.72, 0, 1);
}
.plan-cols-leave-active {
  transition: opacity 0.16s ease, transform 0.16s ease;
}
.plan-cols-enter-from {
  opacity: 0;
  transform: translateX(-8px) scale(0.9);
}
.plan-cols-leave-to {
  opacity: 0;
  transform: translateX(8px) scale(0.9);
}
.nav-chevron {
  margin-left: auto;
  width: 8px;
  height: 8px;
  border-right: 1.5px solid var(--text-tertiary);
  border-bottom: 1.5px solid var(--text-tertiary);
  transform: rotate(45deg);
  transition: transform var(--transition-fast), opacity 0.1s ease;
  opacity: 1;
  flex-shrink: 0;
}
.nav-chevron.open {
  transform: rotate(-135deg);
}
.nav-children {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding-left: var(--space-3);
  /* 展开/收起动画：max-height 过渡 */
  animation: child-in var(--transition-normal);
}
@keyframes child-in {
  from { opacity: 0; transform: translateY(-4px); }
  to { opacity: 1; transform: translateY(0); }
}
.nav-child {
  min-height: var(--sidebar-child-control-height);
  padding: var(--space-1) var(--space-3);
  font-size: var(--text-sm);
}
.child-icon {
  opacity: 0.6;
}

.sidebar-bottom {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding-top: var(--space-2);
  border-top: 1px solid var(--divider-color);
  margin-top: var(--space-2);
  flex-shrink: 0;
}

.bottom-item {
  font-size: var(--text-sm);
  opacity: 0.85;
}

.bottom-item:hover {
  opacity: 1;
}

.theme-toggle {
  font-family: inherit;
}

.version-label {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-1) var(--space-3);
  border: none;
  background: transparent;
  font-size: 10px;
  font-weight: var(--font-medium);
  color: var(--text-quaternary);
  text-align: left;
  letter-spacing: 0.02em;
  cursor: pointer;
  font-family: inherit;
  border-radius: var(--radius-xs);
  /* 收起/展开：跟随文字动画 + 自身高度联动 */
  max-height: 48px;
  overflow: hidden;
  opacity: 1;
  transition: background var(--transition-fast), color var(--transition-fast),
    opacity 0.2s cubic-bezier(0.32, 0.72, 0, 1) 0.12s,
    transform 0.22s cubic-bezier(0.32, 0.72, 0, 1) 0.12s,
    max-height var(--transition-slow), padding var(--transition-slow);
}

.version-label:hover:not(:disabled) {
  background: var(--sidebar-item-hover);
  color: var(--text-secondary);
}

/* 系统开启「减弱动态效果」时关闭全部收起/展开过渡，直接切换 */
@media (prefers-reduced-motion: reduce) {
  .sidebar,
  .brand-text,
  .nav-label,
  .nav-badge,
  .nav-chevron,
  .version-label,
  .plan-cols,
  .nav-children {
    transition: none;
    animation: none;
  }
}
</style>
