<script setup lang="ts">
withDefaults(defineProps<{
  size?: number;
  label?: string;
}>(), {
  size: 24,
  label: "",
});
</script>

<template>
  <div class="loading-wrapper">
    <div class="spinner" :style="{ width: `${size}px`, height: `${size}px` }" />
    <span v-if="label" class="loading-label">{{ label }}</span>
  </div>
</template>

<style scoped>
.loading-wrapper {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-3);
  padding: var(--space-8);
}

/* Apple-style: subtle, quiet spinner */
.spinner {
  border: 2px solid var(--bg-tertiary);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: spin 0.7s linear infinite;
}

.loading-label {
  font-size: var(--text-sm);
  color: var(--text-tertiary);
}

@keyframes spin {
  to { transform: rotate(360deg); }
}
</style>
