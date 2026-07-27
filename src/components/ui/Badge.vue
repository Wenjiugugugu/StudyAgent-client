<script setup lang="ts">
withDefaults(defineProps<{
  variant?: "default" | "success" | "warning" | "danger" | "info" | "math" | "english" | "politics" | "professional";
  size?: "sm" | "md";
}>(), {
  variant: "default",
  size: "sm",
});
</script>

<template>
  <span class="ui-badge" :class="[variant, size]">
    <slot />
  </span>
</template>

<style scoped>
/* Apple design library: rounded-full, restrained color, quiet labels */
.ui-badge {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  font-weight: var(--font-medium);
  border-radius: var(--radius-full);
  white-space: nowrap;
  letter-spacing: -0.01em;
}

.sm { font-size: var(--text-xs); padding: 2px 8px; }
.md { font-size: var(--text-sm); padding: 4px 12px; }

.default {
  background: var(--bg-tertiary);
  color: var(--text-secondary);
}

.success {
  background: var(--color-success-subtle);
  color: var(--color-success);
}

.warning {
  background: var(--color-warning-subtle);
  color: var(--color-warning);
}

.danger {
  background: var(--color-danger-subtle);
  color: var(--color-danger);
}

.info {
  background: var(--color-info-subtle);
  color: var(--color-info);
}

.math {
  background: var(--color-math-subtle);
  color: var(--color-math);
}

.english {
  background: var(--color-english-subtle);
  color: var(--color-english);
}

.politics {
  background: var(--color-politics-subtle);
  color: var(--color-politics);
}

.professional {
  background: var(--color-professional-subtle);
  color: var(--color-professional);
}
</style>
