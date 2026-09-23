<script setup lang="ts">
/**
 * 设置页 — 薄壳
 *
 * 阶段3重构后仅作为路由入口，实际逻辑与视图已迁移至
 * src/features/settings/SettingsPage.vue 及其 Section 子组件 / composables。
 * 路由（/settings）仍指向本文件，保持对外路径不变。
 */
import SettingsPage from "@/features/settings/SettingsPage.vue";
</script>

<template>
  <SettingsPage />
</template>
