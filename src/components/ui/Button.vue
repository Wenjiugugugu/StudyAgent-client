<script setup lang="ts">
withDefaults(defineProps<{
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md" | "lg";
  disabled?: boolean;
  loading?: boolean;
  icon?: boolean;
}>(), {
  variant: "primary",
  size: "md",
  disabled: false,
  loading: false,
  icon: false,
});
</script>

<template>
  <button
    class="ui-button"
    :class="[variant, size, { 'icon-only': icon, disabled, loading }]"
    :disabled="disabled || loading"
    :aria-busy="loading"
  >
    <span v-if="loading" class="btn-spinner" />
    <slot />
  </button>
</template>

<style scoped>
/* Apple design library: capsule-shaped actions, restrained hierarchy */
.ui-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  border: none;
  font-family: inherit;
  font-weight: var(--font-label);
  cursor: pointer;
  transition: background-color var(--transition-normal), box-shadow var(--transition-normal), transform var(--transition-fast), opacity var(--transition-normal);
  white-space: nowrap;
  /* Fully pill-shaped per Apple design library */
  border-radius: var(--radius-full);
  letter-spacing: -0.01em;
}

.ui-button:active:not(:disabled) {
  transform: scale(0.97);
}

.ui-button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* Variants — capsule CTA hierarchy */
.primary {
  background: var(--accent);
  color: var(--text-on-accent);
  font-weight: var(--font-semibold);
}
.primary:hover:not(:disabled) {
  background: var(--accent-hover);
}
.primary:active:not(:disabled) {
  background: var(--accent-pressed);
}

.secondary {
  background: var(--bg-tertiary);
  color: var(--text-primary);
  border: 1px solid var(--border-color);
}
.secondary:hover:not(:disabled) {
  background: var(--sidebar-item-hover);
  border-color: var(--border-color-strong);
}

.ghost {
  background: transparent;
  color: var(--text-secondary);
}
.ghost:hover:not(:disabled) {
  background: var(--sidebar-item-hover);
  color: var(--text-primary);
}

.danger {
  background: var(--color-danger);
  color: white;
}
.danger:hover:not(:disabled) {
  opacity: 0.9;
}

/* Sizes — tactile capsule proportions */
.sm { padding: var(--space-1) var(--space-3); font-size: var(--text-sm); min-height: 28px; }
.md { padding: var(--space-2) var(--space-5); font-size: var(--text-sm); min-height: 36px; }
.lg { padding: var(--space-3) var(--space-6); font-size: var(--text-base); min-height: 44px; }

.icon-only {
  padding: var(--space-2);
  width: 36px;
  height: 36px;
}

/* Spinner */
.btn-spinner {
  width: 14px;
  height: 14px;
  border: 2px solid currentColor;
  border-top-color: transparent;
  border-radius: 50%;
  animation: spin 0.6s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}
</style>
