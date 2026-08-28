/**
 * 设置页 — 左侧快速导航与区块滚动逻辑
 *
 * 承载导航项列表、当前活跃区块、滚动定位与 IntersectionObserver 联动，
 * 以及卸载时观察器清理（原 SettingsView 中 navSections / scrollToSection / initSectionObserver）。
 */
import { ref } from "vue";
import { onBeforeUnmount } from "vue";
import type { Component } from "vue";
import { User, PowerOff, Palette, Target, Clock, Gauge, BookOpen, Bot, Cloud, FolderOpen, RefreshCw } from "lucide-vue-next";

export interface NavSection {
  id: string;
  label: string;
  icon: Component;
}

export function useSectionNavigation() {
  const navSections: NavSection[] = [
    { id: "personal", label: "个人信息", icon: User },
    { id: "general", label: "通用", icon: PowerOff },
    { id: "appearance", label: "外观", icon: Palette },
    { id: "goals", label: "学习目标", icon: Target },
    { id: "schedule", label: "学习时间", icon: Clock },
    { id: "rhythm", label: "学习节奏", icon: Gauge },
    { id: "textbooks", label: "教材", icon: BookOpen },
    { id: "ai-provider", label: "AI Provider", icon: Bot },
    { id: "dida-sync", label: "滴答同步", icon: Cloud },
    { id: "storage", label: "存储", icon: FolderOpen },
    { id: "update", label: "检查更新", icon: RefreshCw },
  ];

  const activeSection = ref("personal");

  /**
   * 滚动到某个设置区块。
   * 注：刻意不使用 scrollIntoView() 避免其在 Tauri WebView2（body html overflow:hidden）
   * 环境下递归触发 html/documentElement 级「伪滚动」，导致整个应用视口向上错位、
   * 顶栏/侧边栏被挤出视口。此处直接在最近可滚动容器 .content-body 上做精确 scrollTo。
   */
  function scrollToSection(id: string) {
    const el = document.getElementById(`settings-${id}`);
    if (!el) return;
    const scroller = el.closest<HTMLElement>(".content-body") ?? document.querySelector<HTMLElement>(".content-body");
    if (!scroller) {
      // 兜底（极少见）：退化到原 scrollIntoView
      el.scrollIntoView({ behavior: "smooth", block: "start" });
      activeSection.value = id;
      return;
    }
    const scrollerRect = scroller.getBoundingClientRect();
    const elRect = el.getBoundingClientRect();
    // 当前相对 scroller 已滚动到的顶部像素 + 目标与 scroller 的相对顶部差
    const target = Math.max(0, Math.round(scroller.scrollTop + (elRect.top - scrollerRect.top)));
    try {
      scroller.scrollTo({ top: target, behavior: "smooth" });
    } catch {
      // 部分老环境不支持 options 形参
      scroller.scrollTop = target;
    }
    activeSection.value = id;
  }

  function onSectionIntersect(entries: IntersectionObserverEntry[]) {
    for (const entry of entries) {
      if (entry.isIntersecting) {
        const id = entry.target.id.replace("settings-", "");
        activeSection.value = id;
      }
    }
  }

  let sectionObserver: IntersectionObserver | null = null;

  function initSectionObserver() {
    if (sectionObserver) return;
    sectionObserver = new IntersectionObserver(onSectionIntersect, {
      rootMargin: "-20% 0px -60% 0px",
      threshold: 0,
    });
    navSections.forEach((s) => {
      const el = document.getElementById(`settings-${s.id}`);
      if (el) sectionObserver?.observe(el);
    });
  }

  // H34：卸载时断开 IntersectionObserver，避免跨路由内存泄漏
  onBeforeUnmount(() => {
    sectionObserver?.disconnect();
    sectionObserver = null;
  });

  return {
    navSections,
    activeSection,
    scrollToSection,
    initSectionObserver,
  };
}
