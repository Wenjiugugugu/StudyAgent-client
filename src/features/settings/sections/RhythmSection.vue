<script setup lang="ts">
import { computed } from "vue";
import Card from "@/components/ui/Card.vue";
import Select from "@/components/ui/Select.vue";
import DatePicker from "@/components/ui/DatePicker.vue";
import { Gauge } from "lucide-vue-next";
import type { SettingsForm } from "../composables/useSettingsForm";
import type { SubjectActive } from "../types";

const props = defineProps<{
  form: SettingsForm;
  subjectActive: SubjectActive;
  professionalName: string;
}>();

// 每日任务数 = 每日目标学时 ÷ 标准任务粒度（效率系数由后端按上周完成率自校准，前端按 1.0 展示基准值）
const derivedTaskCount = computed(() => {
  const target = Math.max(0, props.form.daily_target_hours || 0);
  const gran = Math.max(0.5, props.form.standard_granularity || 1.5);
  return Math.max(1, Math.min(8, Math.round(target / gran)));
});

// Select 组件以字符串下发值，这里做 number ↔ string 转换
const granularityDisplay = computed<string>({
  get: () => String(props.form.standard_granularity ?? 1.5),
  set: (v: string) => {
    props.form.standard_granularity = Number(v);
  },
});
const GRANULARITY_OPTIONS = [
  { value: "1", label: "1 小时/条" },
  { value: "1.25", label: "1.25 小时/条" },
  { value: "1.5", label: "1.5 小时/条" },
  { value: "2", label: "2 小时/条" },
];
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
          每天任务数（自动计算）
          <span class="field-hint">由「每日目标学时 ÷ 标准任务粒度」得出，调整学时时自动变化</span>
        </label>
        <input
          :value="derivedTaskCount"
          type="number"
          min="1"
          max="8"
          class="form-input"
          readonly
          tabindex="-1"
        />
        <p class="field-hint">
          当前目标学时 {{ form.daily_target_hours }}h ÷ 粒度 {{ form.standard_granularity }}h/条 ≈
          {{ derivedTaskCount }} 个任务（每科约一条；未开始的科目不安排，实际会相应减少）
        </p>
      </div>
      <div class="form-field">
        <label class="form-label">
          标准任务粒度
          <span class="field-hint">（高级设置，默认 1.5h/条 ≈ 2 个番茄钟）</span>
        </label>
        <Select v-model="granularityDisplay">
          <option v-for="opt in GRANULARITY_OPTIONS" :key="opt.value" :value="opt.value">
            {{ opt.label }}
          </option>
        </Select>
        <p class="field-hint">粒度越小任务拆得越细（条数越多），越大越粗（条数越少）。</p>
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
