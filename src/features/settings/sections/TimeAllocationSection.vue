<script setup lang="ts">
/**
 * 设置页 — 学科时间分配
 *
 * 只设定「每日目标学时」（总时长，form.daily_target_hours）与滑块步长（细粒度），
 * 可视化调整每个学科的学习时间占比（百分比，活跃科目合计恒为 100）。
 * 调整任一学科时，其余活跃学科按各自占比等比缩放（largest-remainder 整数守恒）。
 * 保存后影响周计划生成与复盘重排的任务条数分布与各科学时。
 */
import { computed } from "vue";
import Card from "@/components/ui/Card.vue";
import { PieChart } from "lucide-vue-next";
import type { SubjectTimeAllocation } from "@/types/settings";
import type { SubjectKey } from "@/types/state";
import type { StudyState } from "@/types/state";
import type { SettingsForm } from "../composables/useSettingsForm";
import type { SubjectActive } from "../types";
import {
  ALLOCATION_KEYS,
  SLIDER_STEP,
  adjustAllocation,
  deriveFromWeeklyHours,
  normalizeAllocation,
} from "../composables/timeAllocation";

const props = defineProps<{
  form: SettingsForm;
  studyState: StudyState | null;
  subjectActive: SubjectActive;
  professionalName: string;
}>();

/** 各科周学时（state 未加载时为 0，用于推导默认占比的兜底） */
const weekly = computed<Record<SubjectKey, number>>(() => ({
  math: props.studyState?.subjects?.math?.weekly_hours ?? 0,
  english: props.studyState?.subjects?.english?.weekly_hours ?? 0,
  politics: props.studyState?.subjects?.politics?.weekly_hours ?? 0,
  professional: props.studyState?.subjects?.professional?.weekly_hours ?? 0,
}));

/** 当前生效的占比：已配置则归一化存储值；未配置则按周学时推导（null 语义 = 未配置） */
const effective = computed<SubjectTimeAllocation>(() => {
  if (!props.form.subject_time_allocation) {
    return deriveFromWeeklyHours(weekly.value, props.subjectActive);
  }
  return normalizeAllocation(props.form.subject_time_allocation, weekly.value, props.subjectActive);
});

const activeKeys = computed<SubjectKey[]>(() =>
  ALLOCATION_KEYS.filter((k) => props.subjectActive[k]),
);

const subjectLabel = (key: SubjectKey): string => {
  switch (key) {
    case "math":
      return "数学";
    case "english":
      return "英语";
    case "politics":
      return "政治";
    case "professional":
      return props.professionalName || "专业课";
  }
};

/** 每日学时 = 每日目标学时 × 占比 */
const dailyHours = (key: SubjectKey): string => {
  const share = effective.value[key] ?? 0;
  return (((props.form.daily_target_hours || 0) * share) / 100).toFixed(1);
};

/** 每周学时 = 每日学时 × 每周学习天数 */
const weeklyHours = (key: SubjectKey): string => {
  const studyDays = Math.max(0, 7 - (props.form.rest_days?.length ?? 0));
  const share = effective.value[key] ?? 0;
  return (((props.form.daily_target_hours || 0) * share) / 100 * studyDays).toFixed(1);
};

/** 滑块联动：调大该科时其余科目等比缩小，总和恒 100 */
function onSlide(key: SubjectKey, rawValue: string) {
  props.form.subject_time_allocation = adjustAllocation(
    effective.value,
    key,
    Number(rawValue),
    props.subjectActive,
  );
}

/** 恢复默认：置 null，回退为按各科周学时推导（保存时后端同样回退） */
function resetAllocation() {
  props.form.subject_time_allocation = null;
}

/** 各科展示色（浅/深色主题下均可用） */
const SUBJECT_COLORS: Record<SubjectKey, string> = {
  math: "#5b8def",
  english: "#22c55e",
  politics: "#f59e0b",
  professional: "#a78bfa",
};
</script>

<template>
  <Card id="settings-time-allocation" padding="lg" class="settings-section">
    <div class="section-head">
      <div class="section-title">
        <PieChart :size="18" />
        <span>学科时间分配</span>
      </div>
    </div>
    <p class="field-hint alloc-total-hint">
      按占比分配每日约 {{ form.daily_target_hours || 0 }}h 学习时长；调整「每日目标学时」后各科每日学时自动更新；活跃科目合计恒为 100%。
    </p>

    <div v-if="activeKeys.length === 0" class="form-field form-field-full">
      <p class="field-hint">暂无活跃科目，可在引导流程或学习状态中开启科目后再配置占比。</p>
    </div>

    <template v-else>
      <div class="form-grid">
        <div v-for="key in activeKeys" :key="key" class="form-field form-field-full alloc-row">
          <label class="form-label">
            {{ subjectLabel(key) }}
            <span class="field-hint">每日约 {{ dailyHours(key) }}h · 每周约 {{ weeklyHours(key) }}h</span>
          </label>
          <div class="alloc-slider-row">
            <input
              type="range"
              min="0"
              max="100"
              :step="SLIDER_STEP"
              :value="effective[key]"
              class="alloc-slider"
              :style="{ accentColor: SUBJECT_COLORS[key] }"
              @input="onSlide(key, ($event.target as HTMLInputElement).value)"
            />
            <span class="alloc-percent">{{ Math.round(effective[key] ?? 0) }}%</span>
          </div>
        </div>
      </div>

      <!-- 占比可视化条 -->
      <div class="alloc-bar" role="img" aria-label="各科学时占比条形图">
        <div
          v-for="key in activeKeys"
          :key="key"
          class="alloc-bar-seg"
          :style="{
            width: (effective[key] ?? 0) + '%',
            background: SUBJECT_COLORS[key],
          }"
        />
      </div>
      <div class="alloc-legend">
        <span v-for="key in activeKeys" :key="key" class="alloc-legend-item">
          <i class="alloc-legend-dot" :style="{ background: SUBJECT_COLORS[key] }" />
          {{ subjectLabel(key) }} {{ Math.round(effective[key] ?? 0) }}%
        </span>
      </div>

      <div class="form-field form-field-full">
        <div class="alloc-actions">
          <button type="button" class="option-chip" @click="resetAllocation">
            恢复默认（按各科周学时推导）
          </button>
        </div>
        <p class="field-hint">占比为 0 的科目将不安排任务；未到开始日期的科目即使占比大于 0 也不会安排。</p>
      </div>
    </template>
  </Card>
</template>
