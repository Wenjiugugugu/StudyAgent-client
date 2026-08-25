<script setup lang="ts">
import Card from "@/components/ui/Card.vue";
import DatePicker from "@/components/ui/DatePicker.vue";
import { Target, Calendar } from "lucide-vue-next";
import type { SettingsForm } from "../composables/useSettingsForm";

defineProps<{
  form: SettingsForm;
}>();
</script>

<template>
  <!-- 学习目标配置区 -->
  <Card id="settings-goals" padding="lg" class="settings-section">
    <div class="section-head">
      <div class="section-title">
        <Target :size="18" />
        <span>学习目标</span>
      </div>
    </div>
    <div class="form-grid">
      <div class="form-field">
        <label class="form-label">目标分数</label>
        <input
          v-model.number="form.target_score"
          type="number"
          min="0"
          max="500"
          class="form-input"
          placeholder="如：360"
        />
      </div>
      <div class="form-field form-field-full">
        <label class="form-label">
          <Calendar :size="12" class="label-icon" />
          考试日期
        </label>
        <DatePicker v-model="form.exam_date" :clearable="false" />
      </div>
    </div>
  </Card>
</template>
