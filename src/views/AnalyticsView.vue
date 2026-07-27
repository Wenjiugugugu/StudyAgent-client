<script setup lang="ts">
import EmptyState from "@/components/ui/EmptyState.vue";
import { BarChart3 } from "lucide-vue-next";
</script>

<template>
  <div class="analytics-view">
    <EmptyState
      title="Analytics 即将上线"
      description="学习数据分析功能正在开发中，敬请期待。"
    >
      <template #actions>
        <div class="coming-soon-hint">
          <BarChart3 :size="20" />
          <span>功能开发中</span>
        </div>
      </template>
    </EmptyState>
  </div>
</template>

<style scoped>
.analytics-view {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 100%;
  padding: var(--space-8);
}

.coming-soon-hint {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--text-tertiary);
  font-size: var(--text-sm);
}
</style>
