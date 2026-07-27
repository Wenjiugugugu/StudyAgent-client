<script setup lang="ts">
withDefaults(defineProps<{
  padding?: "sm" | "md" | "lg";
  hoverable?: boolean;
  noShadow?: boolean;
  surface?: "default" | "1" | "2" | "3" | "accent";
}>(), {
  padding: "md",
  hoverable: false,
  noShadow: false,
  surface: "default",
});
</script>

<template>
  <div
    class="ui-card"
    :class="[
      `padding-${padding}`,
      `surface-${surface}`,
      { hoverable, 'no-shadow': noShadow }
    ]"
  >
    <slot />
  </div>
</template>

<style scoped>
/* Apple design library: quiet containers defined by soft borders and spacing.
   Different modules use layered surface tints instead of all-white cards.
   Hover implies interactivity via gentle lift + soft shadow. */
.ui-card {
  border: 1px solid var(--border-color);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
  transition: box-shadow var(--transition-normal), transform var(--transition-normal), border-color var(--transition-normal);
}

/* Surface tints — layered backgrounds for visual hierarchy without dividers */
.surface-default { background: var(--bg-elevated); }
.surface-1 { background: var(--surface-1); }
.surface-2 { background: var(--surface-2); }
.surface-3 { background: var(--surface-3); }
.surface-accent { background: var(--surface-accent); }

.ui-card.no-shadow {
  box-shadow: none;
  border: 1px solid var(--border-color);
}

.ui-card.hoverable:hover {
  box-shadow: var(--shadow-md);
  transform: translateY(-2px);
  border-color: var(--border-color-strong);
}

.padding-sm { padding: var(--space-4); }
.padding-md { padding: var(--space-6); }
.padding-lg { padding: var(--space-8); }
</style>
