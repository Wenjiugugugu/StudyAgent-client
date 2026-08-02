<script setup lang="ts">
import { ref, watch, nextTick } from "vue";
import { useRoute } from "vue-router";
import { useTheme } from "@/composables/useTheme";
import { useSettingsStore } from "@/stores/settings";
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
} from "lucide-vue-next";

const { theme, toggleTheme } = useTheme();
const settingsStore = useSettingsStore();
const route = useRoute();

const navRef = ref<HTMLElement | null>(null);
const indicatorStyle = ref<{ transform: string; height: string; opacity: number }>({
  transform: "translateY(0px)",
  height: "0px",
  opacity: 0,
});

function updateIndicator() {
  nextTick(() => {
    if (!navRef.value) return;
    const active = navRef.value.querySelector(".nav-item.active") as HTMLElement | null;
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

watch(() => route.path, updateIndicator, { immediate: true });

interface NavItem {
  name: string;
  label: string;
  icon: any;
  path: string;
  reserved?: boolean;
}

const navItems: NavItem[] = [
  { name: "dashboard", label: "工作台", icon: LayoutDashboard, path: "/dashboard" },
  { name: "today", label: "计划", icon: Calendar, path: "/today" },
  { name: "week-plan", label: "周计划", icon: CalendarDays, path: "/week-plan" },
  { name: "history-plans", label: "历史计划", icon: History, path: "/history-plans" },
  { name: "textbooks", label: "教材", icon: BookOpen, path: "/textbooks" },
  { name: "review", label: "复盘", icon: ClipboardCheck, path: "/review" },
  { name: "analytics", label: "分析", icon: BarChart3, path: "/analytics" },
  { name: "timeline", label: "时间线", icon: GitBranch, path: "/timeline", reserved: true },
];

// 当前版本号（用于侧边栏底部展示）
const APP_VERSION = "0.3.2";
</script>

<template>
  <aside class="sidebar">
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
      <router-link
        v-for="item in navItems"
        :key="item.name"
        :to="item.path"
        class="nav-item"
        :class="{ reserved: item.reserved }"
        active-class="active"
      >
        <component :is="item.icon" :size="19" :stroke-width="1.5" class="nav-icon" />
        <span class="nav-label">{{ item.label }}</span>
      </router-link>
    </nav>

    <!-- Bottom Section -->
    <div class="sidebar-bottom">
      <router-link
        to="/debug"
        class="nav-item bottom-item"
        active-class="active"
      >
        <Bug :size="19" :stroke-width="1.5" class="nav-icon" />
        <span class="nav-label">调试</span>
      </router-link>

      <router-link
        to="/settings"
        class="nav-item bottom-item"
        active-class="active"
      >
        <Settings :size="19" :stroke-width="1.5" class="nav-icon" />
        <span class="nav-label">设置</span>
      </router-link>

      <button class="nav-item theme-toggle" @click="toggleTheme">
        <component :is="theme === 'dark' ? SunMedium : Moon" :size="19" :stroke-width="1.5" class="nav-icon" />
        <span class="nav-label">{{ theme === "dark" ? "浅色" : "深色" }}</span>
      </button>

      <router-link
        to="/settings#settings-update"
        class="version-label"
        title="前往设置页检查更新"
      >
        <span>Beta {{ APP_VERSION }}</span>
      </router-link>
    </div>
  </aside>
</template>

<style scoped>
/* Apple design library: frosted glass sidebar — the signature macOS material.
   Deeper translucent base + heavy backdrop blur reads clearly against the
   lighter content area; faint inner highlight simulates glass edge. */
.sidebar {
  width: var(--sidebar-width);
  min-width: var(--sidebar-width);
  height: 100%;
  background: var(--sidebar-bg);
  backdrop-filter: saturate(200%) blur(30px);
  -webkit-backdrop-filter: saturate(200%) blur(30px);
  display: flex;
  flex-direction: column;
  padding: var(--space-3) var(--space-2) var(--space-3);
  border-right: 1px solid var(--border-color);
  box-shadow: inset -1px 0 0 rgba(255, 255, 255, 0.4);
  user-select: none;
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
  transition: transform 0.25s cubic-bezier(0.32, 0.72, 0, 1), height 0.2s ease, opacity 0.2s ease;
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
  padding: var(--space-2) var(--space-3);
  border: none;
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
  transition: background var(--transition-fast), color var(--transition-fast);
}

.version-label:hover:not(:disabled) {
  background: var(--sidebar-item-hover);
  color: var(--text-secondary);
}
</style>
