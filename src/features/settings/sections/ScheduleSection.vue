<script setup lang="ts">
import Card from "@/components/ui/Card.vue";
import TimePicker from "@/components/ui/TimePicker.vue";
import { Clock } from "lucide-vue-next";
import type { SettingsForm } from "../composables/useSettingsForm";

defineProps<{
  form: SettingsForm;
  weekdayOptions: string[];
  toggleRestDay: (day: string) => void;
  computedStudyDays: number;
  isStudyDaysValid: boolean;
}>();
</script>

<template>
  <!-- 学习时间配置区（仅时间相关） -->
  <Card id="settings-schedule" padding="lg" class="settings-section">
    <div class="section-head">
      <div class="section-title">
        <Clock :size="18" />
        <span>学习时间</span>
      </div>
    </div>
    <div class="form-grid">
      <div class="form-field">
        <label class="form-label">每日开始时间</label>
        <TimePicker v-model="form.start_time" :minute-step="15" />
      </div>
      <div class="form-field">
        <label class="form-label">每日结束时间</label>
        <TimePicker v-model="form.end_time" :minute-step="15" />
      </div>
      <div class="form-field">
        <label class="form-label">每日目标学时</label>
        <input v-model.number="form.daily_target_hours" type="number" step="0.5" min="0" class="form-input" />
      </div>
      <div class="form-field">
        <label class="form-label">复盘提醒时间</label>
        <TimePicker v-model="form.review_reminder_time" :minute-step="15" />
      </div>
      <div class="form-field form-field-full">
        <label class="form-label">每周休息日（可多选）</label>
        <div class="option-grid">
          <button
            v-for="day in weekdayOptions"
            :key="day"
            type="button"
            class="option-chip"
            :class="{ active: form.rest_days.includes(day) }"
            @click="toggleRestDay(day)"
          >
            {{ day }}
          </button>
        </div>
        <p class="field-hint">选择休息日后，学习天数会自动调整为剩余天数。</p>
      </div>
      <div class="form-field">
        <label class="form-label">
          每周学习天数
          <span class="field-hint">（自动计算：7 - 休息天数）</span>
        </label>
        <input
          :value="computedStudyDays"
          type="number"
          min="1"
          max="7"
          class="form-input"
          :class="{ invalid: !isStudyDaysValid }"
          readonly
          tabindex="-1"
        />
        <div v-if="!isStudyDaysValid" class="error-text">
          学习天数不能为 0，请至少选择 1 天作为学习日
        </div>
      </div>
    </div>
  </Card>
</template>
