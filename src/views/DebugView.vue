<script setup lang="ts">
/**
 * 调试页 — 薄壳
 *
 * 阶段4重构后仅作为路由入口，实际逻辑与视图已迁移至
 * src/features/debug/DebugPage.vue 及其面板组件 / composables。
 * 路由（/debug）仍指向本文件，保持对外路径不变。
 */
import DebugPage from "@/features/debug/DebugPage.vue";
</script>

<template>
  <DebugPage />
</template>
