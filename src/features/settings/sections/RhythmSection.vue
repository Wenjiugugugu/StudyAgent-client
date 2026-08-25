<script setup lang="ts">
import Card from "@/components/ui/Card.vue";
import DatePicker from "@/components/ui/DatePicker.vue";
import { Gauge } from "lucide-vue-next";
import type { SettingsForm } from "../composables/useSettingsForm";
import type { SubjectActive } from "../types";

defineProps<{
  form: SettingsForm;
  subjectActive: SubjectActive;
  professionalName: string;
}>();
</script>

<template>
  <!-- 学习节奏配置区（任务数/总结任务/各科开始日期） -->
  <Card id="settings-rhythm" padding="lg" class="settings-section">
    <div class="section-head">
      <div class="section-title">
        <Gauge :size="18" />
        <span>学习节奏</span>
      </div>
    </div>
    <div class="form-grid">
      <div class="form-field">
        <label class="form-label">
          每天安排多少任务
          <span class="field-hint">（每科约一条；未开始的科目不安排，相应减少当日任务数）</span>
        </label>
        <input
          v-model.number="form.daily_task_count"
          type="number"
          min="1"
          max="8"
          class="form-input"
        />
        <p class="field-hint">默认 3-4 个。例如政治未到开始日期时，当天不会排政治任务。</p>
      </div>
      <div class="form-field form-field-full">
        <label class="form-label">是否安排总结/复习任务</label>
        <div class="option-grid option-grid-2">
          <button
            type="button"
            class="option-chip"
            :class="{ active: form.enable_review_tasks !== false }"
            @click="form.enable_review_tasks = true"
          >
            安排（推荐）
          </button>
          <button
            type="button"
            class="option-chip"
            :class="{ active: form.enable_review_tasks === false }"
            @click="form.enable_review_tasks = false"
          >
            只推进新知识点
          </button>
        </div>
        <p class="field-hint">关闭后 AI 不会安排"回顾"/"总结"/"复习"类任务，适合希望持续向前推进的用户。</p>
      </div>
      <div class="form-field form-field-full">
        <label class="form-label">记录学习时长</label>
        <div class="option-grid option-grid-2">
          <button
            type="button"
            class="option-chip"
            :class="{ active: form.enable_time_tracking }"
            @click="form.enable_time_tracking = true"
          >
            开启
          </button>
          <button
            type="button"
            class="option-chip"
            :class="{ active: !form.enable_time_tracking }"
            @click="form.enable_time_tracking = false"
          >
            不开启（默认）
          </button>
        </div>
        <p class="field-hint">开启后任务卡显示开始/暂停按钮，记录每项任务的专注时长；关闭时只关注完成内容。</p>
      </div>
      <div class="form-field form-field-full">
        <label class="form-label">
          各科开始学习日期
          <span class="field-hint">（留空表示立即开始；未到日期前不为该科安排任务）</span>
        </label>
        <div class="subject-start-grid">
          <div v-if="subjectActive.math" class="subject-start-item">
            <span class="subject-start-label">数学</span>
            <DatePicker v-model="form.subject_start_dates.math" placeholder="立即开始" />
          </div>
          <div v-if="subjectActive.english" class="subject-start-item">
            <span class="subject-start-label">英语</span>
            <DatePicker v-model="form.subject_start_dates.english" placeholder="立即开始" />
          </div>
          <div v-if="subjectActive.politics" class="subject-start-item">
            <span class="subject-start-label">政治</span>
            <DatePicker v-model="form.subject_start_dates.politics" placeholder="立即开始" />
          </div>
          <div v-if="subjectActive.professional" class="subject-start-item">
            <span class="subject-start-label">{{ professionalName }}</span>
            <DatePicker v-model="form.subject_start_dates.professional" placeholder="立即开始" />
          </div>
        </div>
        <p class="field-hint">例如政治计划 8 月中旬开始，可将政治开始日期设为 2026-08-15，此前不会安排政治任务。</p>
      </div>
    </div>
  </Card>
</template>
