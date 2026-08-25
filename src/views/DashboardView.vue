<script setup lang="ts">
/**
 * 工作台 — 薄壳
 *
 * 阶段5重构后仅作为路由入口，实际逻辑与视图已迁移至
 * src/features/dashboard/DashboardPage.vue 及其 composables / components。
 * 路由（/）仍指向本文件，保持对外路径不变。
 */
import DashboardPage from "@/features/dashboard/DashboardPage.vue";
</script>

<template>
  <DashboardPage />
</template>
