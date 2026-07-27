<script setup lang="ts">
import EmptyState from "@/components/ui/EmptyState.vue";
import { GitBranch } from "lucide-vue-next";
</script>

<template>
  <div class="timeline-view">
    <EmptyState
      title="Timeline 即将上线"
      description="学习时间线功能正在开发中，敬请期待。"
    >
      <template #actions>
        <div class="coming-soon-hint">
          <GitBranch :size="20" />
          <span>功能开发中</span>
        </div>
      </template>
    </EmptyState>
  </div>
</template>

<style scoped>
.timeline-view {
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
