<script setup lang="ts">
/**
 * 工作台 — Hero 头部
 *
 * 展示问候语、用户称呼、今日日期与「距考研剩余天数」倒计时。
 */
import { Calendar } from "lucide-vue-next";

defineProps<{
  greeting: string;
  displayName: string;
  dateLabel: string;
  remainingDays: number;
  showGreeting: boolean;
}>();
</script>

<template>
  <header class="hero">
    <div class="hero-text">
      <h1 v-if="showGreeting" class="hero-name">
        {{ displayName ? `${greeting}，${displayName}` : greeting }}
      </h1>
      <h1 v-else class="hero-name">{{ displayName }}</h1>
      <span class="hero-date">
        <Calendar :size="12" />
        {{ dateLabel }}
      </span>
    </div>
    <div class="hero-countdown">
      <span class="countdown-number">{{ remainingDays }}</span>
      <span class="countdown-unit">天后考研</span>
    </div>
  </header>
</template>
