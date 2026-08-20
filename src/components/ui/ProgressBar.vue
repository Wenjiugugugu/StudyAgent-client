<script setup lang="ts">
import { computed } from "vue";

const props = withDefaults(defineProps<{
  value: number;
  max?: number;
  variant?: "default" | "success" | "warning" | "danger" | "math" | "english" | "politics" | "professional";
  size?: "sm" | "md" | "lg";
  showLabel?: boolean;
}>(), {
  max: 100,
  variant: "default",
  size: "md",
  showLabel: false,
});

const percentage = computed(() => {
  const val = Math.min(Math.max(0, (props.value / props.max) * 100), 100);
  return Math.round(val);
});
</script>

<template>
  <div class="progress-wrapper">
    <div class="progress-track" :class="size">
      <div
        class="progress-fill"
        :class="variant"
        :style="{ width: `${percentage}%` }"
      />
    </div>
    <span v-if="showLabel" class="progress-label">{{ percentage }}%</span>
  </div>
</template>

<style scoped>
.progress-wrapper {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  width: 100%;
}

/* Apple-style: thin, rounded-full track */
.progress-track {
  flex: 1;
  background: var(--bg-tertiary);
  border-radius: var(--radius-full);
  overflow: hidden;
}

.sm { height: 4px; }
.md { height: 6px; }
.lg { height: 8px; }

.progress-fill {
  height: 100%;
  border-radius: var(--radius-full);
  transition: width var(--transition-slow);
  background: var(--accent);
}

.progress-fill.success { background: var(--color-success); }
.progress-fill.warning { background: var(--color-warning); }
.progress-fill.danger { background: var(--color-danger); }
.progress-fill.math { background: var(--color-math); }
.progress-fill.english { background: var(--color-english); }
.progress-fill.politics { background: var(--color-politics); }
.progress-fill.professional { background: var(--color-professional); }

.progress-label {
  font-size: var(--text-xs);
  color: var(--text-secondary);
  font-weight: var(--font-medium);
  min-width: 36px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}
</style>
