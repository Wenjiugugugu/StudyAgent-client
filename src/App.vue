<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useSettingsStore } from "@/stores/settings";
import { useAssistantStore } from "@/stores/assistant";
import { useTheme } from "@/composables/useTheme";
import AppLayout from "@/layouts/AppLayout.vue";

const route = useRoute();
const router = useRouter();
const settingsStore = useSettingsStore();
const assistantStore = useAssistantStore();
useTheme();

// 独立路由（如引导页）不套用 AppLayout，全屏渲染
const isStandalone = computed(() => route.meta.standalone === true);

onMounted(async () => {
  await settingsStore.load();

  // 引导状态检查：未完成则跳转引导页
  if (!settingsStore.onboardingCompleted && route.path !== "/onboarding") {
    router.replace("/onboarding");
    return;
  }

  assistantStore.setContext({ current_view: "dashboard" });
});
</script>

<template>
  <router-view v-if="isStandalone" />
  <AppLayout v-else />
</template>
