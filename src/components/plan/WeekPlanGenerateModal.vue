<script setup lang="ts">
/**
 * 「生成本周计划」配置弹窗
 *
 * 原「每周计划」页下线后，生成前需要用户确认的三类信息统一迁移到这里，
 * 由日计划页（TodayView）的「生成本周计划」入口调用：
 *   1. 上周报告 —— 上周已学天数 / 完成任务 / 平均完成率 / 实际学时，作为本周难度参考
 *   2. 本周任务量调整 —— 相对上周增加 / 不变 / 减少（含幅度与备注）
 *   3. 特殊情况排除日 —— 勾选本周不学习的日期，AI 把任务量分摊到其他学习日
 *
 * 生成成功后 emit("generated", plan)，由父组件刷新日计划。
 */
import { computed, ref, watch } from "vue";
import * as api from "@/api";
import { useSettingsStore } from "@/stores/settings";
import { todayString } from "@/utils/date";
import Modal from "@/components/ui/Modal.vue";
import Button from "@/components/ui/Button.vue";
import Select from "@/components/ui/Select.vue";
import Checkbox from "@/components/ui/Checkbox.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import { CalendarDays, Sparkles, TrendingUp } from "lucide-vue-next";
import type {
  WeekPlan,
  PlanSummary,
  ExcludedDay,
  ExcludedReasonType,
  WorkloadDirection,
  WorkloadLevel,
} from "@/types";

const props = defineProps<{
  /** 是否显示 */
  open: boolean;
  /** 目标周周一（YYYY-MM-DD） */
  weekStart: string;
}>();

const emit = defineEmits<{
  (e: "close"): void;
  (e: "generated", plan: WeekPlan): void;
}>();

const settingsStore = useSettingsStore();

// ── 表单状态 ──
const wlDirection = ref<WorkloadDirection>("unchanged");
const wlLevel = ref<WorkloadLevel>("small");
const wlNote = ref("");
/** 生成前勾选的排除日：date -> 配置 */
const configExcluded = ref<Record<string, { reason_type: ExcludedReasonType; note: string }>>({});

// ── 上周报告 ──
const prevWeekSummaries = ref<PlanSummary[]>([]);
const prevWeekReportLoading = ref(false);

// ── 生成 ──
const generating = ref(false);
const generateError = ref("");

// ── 日期工具（本地时区安全，12:00 锚点避免跨时区偏移）──
function addDays(dateStr: string, n: number): string {
  const [y, m, d] = dateStr.split("-").map(Number);
  const dt = new Date(y, m - 1, d, 12, 0, 0);
  dt.setDate(dt.getDate() + n);
  const yy = dt.getFullYear();
  const mm = String(dt.getMonth() + 1).padStart(2, "0");
  const dd = String(dt.getDate()).padStart(2, "0");
  return `${yy}-${mm}-${dd}`;
}

function weekdayFullName(dateStr: string): string {
  const weekdays = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
  const d = new Date(`${dateStr}T12:00:00`);
  return weekdays[d.getDay()];
}

function weekdayShort(dateStr: string): string {
  const weekdays = ["日", "一", "二", "三", "四", "五", "六"];
  const d = new Date(`${dateStr}T12:00:00`);
  return weekdays[d.getDay()];
}

function isUserRestDay(dateStr: string): boolean {
  const restDays = settingsStore.settings?.study_schedule?.rest_days ?? ["周日"];
  return restDays.includes(weekdayFullName(dateStr));
}

const today = computed(() => todayString());

// 是否启用「记录学习时长」：关闭时隐藏学时相关展示
const timeTrackingEnabled = computed(
  () => !!settingsStore.settings?.study_schedule?.enable_time_tracking
);

/** 上周报告：按任务数加权计算平均完成率（同周计划页口径） */
const prevWeekReport = computed(() => {
  const sums = prevWeekSummaries.value;
  if (!sums.length) return null;
  const studyDays = sums.filter((s) => s.has_plan || s.has_review);
  const totalPlanned = sums.reduce((a, s) => a + (s.planned_tasks ?? 0), 0);
  const totalCompleted = sums.reduce((a, s) => a + (s.completed_tasks ?? 0), 0);
  const totalActualHours = sums.reduce((a, s) => a + (s.actual_hours ?? 0), 0);
  const reviewedDays = sums.filter((s) => s.has_review);
  // 不能对每日完成率做算术平均：任务少但全完成的天会等权拉高整体
  const avgCompletion =
    totalPlanned > 0
      ? Math.round((totalCompleted / totalPlanned) * 100)
      : reviewedDays.length
        ? Math.round(reviewedDays.reduce((a, s) => a + (s.completion_rate ?? 0), 0) / reviewedDays.length)
        : 0;
  return {
    studyDays: studyDays.length,
    totalPlanned,
    totalCompleted,
    totalActualHours,
    avgCompletion,
    reviewedDays: reviewedDays.length,
    days: sums,
  };
});

const prevReportDayLabel = computed(() =>
  prevWeekReport.value ? `${prevWeekReport.value.studyDays}/7` : "0/7"
);

/** 可勾选的排除日候选：目标周内今天及之后、且非用户休息日 */
const configExcludeCandidates = computed(() => {
  const candidates: { date: string; weekday: string; isToday: boolean }[] = [];
  for (let i = 0; i < 7; i++) {
    const date = addDays(props.weekStart, i);
    if (date < today.value) continue;
    if (isUserRestDay(date)) continue;
    candidates.push({ date, weekday: weekdayShort(date), isToday: date === today.value });
  }
  return candidates;
});

function wlDirectionLabel(d: WorkloadDirection): string {
  return { increase: "增加", unchanged: "不变", decrease: "减少" }[d];
}

function completionVariant(rate: number): string {
  if (rate >= 100) return "success";
  if (rate >= 50) return "warning";
  return "danger";
}

async function loadPrevWeekReport() {
  prevWeekReportLoading.value = true;
  try {
    prevWeekSummaries.value = await api
      .getWeekSummaries(addDays(props.weekStart, -7))
      .catch(() => [] as PlanSummary[]);
  } finally {
    prevWeekReportLoading.value = false;
  }
}

function resetForm() {
  wlDirection.value = "unchanged";
  wlLevel.value = "small";
  wlNote.value = "";
  configExcluded.value = {};
  generateError.value = "";
}

function toggleConfigExclude(date: string) {
  if (configExcluded.value[date]) {
    const next = { ...configExcluded.value };
    delete next[date];
    configExcluded.value = next;
  } else {
    configExcluded.value = {
      ...configExcluded.value,
      [date]: { reason_type: "travel", note: "" },
    };
  }
}

/** 确认生成：携带排除日 + 任务量调整 */
async function confirmGenerate() {
  if (generating.value) return;
  generating.value = true;
  generateError.value = "";
  try {
    const excludedDays: ExcludedDay[] = Object.entries(configExcluded.value).map(
      ([date, cfg]) => ({
        date,
        reason_type: cfg.reason_type,
        note: cfg.note.trim() || undefined,
      })
    );
    const workloadAdjustment =
      wlDirection.value === "unchanged"
        ? undefined
        : {
            direction: wlDirection.value,
            level: wlLevel.value,
            note: wlNote.value.trim() || undefined,
          };
    const plan = await api.generateWeekPlan(props.weekStart, excludedDays, workloadAdjustment);
    emit("generated", plan);
    emit("close");
  } catch (e) {
    generateError.value = e instanceof Error ? e.message : String(e);
  } finally {
    generating.value = false;
  }
}

// 打开弹窗时重置表单并加载上周数据
watch(
  () => props.open,
  (open) => {
    if (!open) return;
    resetForm();
    loadPrevWeekReport();
  }
);
</script>

<template>
  <Modal
    :open="open"
    title="本周计划配置"
    :width="560"
    :close-on-overlay="!generating"
    :close-on-esc="!generating"
    :show-close="!generating"
    @close="!generating && emit('close')"
  >
    <div class="config-form">
      <!-- 上周报告 -->
      <section class="config-section">
        <h4 class="config-section-title">
          <TrendingUp :size="14" />
          上周报告
        </h4>
        <LoadingSpinner v-if="prevWeekReportLoading" :size="20" label="加载上周数据…" />
        <div v-else-if="prevWeekReport" class="prev-report">
          <div class="prev-report-grid">
            <div class="prev-stat">
              <span class="prev-stat-value">{{ prevReportDayLabel }}</span>
              <span class="prev-stat-label">已学天数</span>
            </div>
            <div class="prev-stat">
              <span class="prev-stat-value">
                {{ prevWeekReport.totalCompleted }}/{{ prevWeekReport.totalPlanned }}
              </span>
              <span class="prev-stat-label">完成任务</span>
            </div>
            <div class="prev-stat">
              <span
                class="prev-stat-value"
                :class="completionVariant(prevWeekReport.avgCompletion)"
              >
                {{ prevWeekReport.avgCompletion }}%
              </span>
              <span class="prev-stat-label">平均完成率</span>
            </div>
            <div v-if="timeTrackingEnabled" class="prev-stat">
              <span class="prev-stat-value">{{ prevWeekReport.totalActualHours.toFixed(1) }}h</span>
              <span class="prev-stat-label">实际学时</span>
            </div>
          </div>
          <div class="prev-report-days">
            <span
              v-for="d in prevWeekReport.days"
              :key="d.date"
              class="prev-day-chip"
              :class="{
                studied: d.has_plan || d.has_review,
                done: d.has_review && d.completion_rate >= 100,
                rest: d.is_rest_day,
              }"
              :title="`${d.date}：${d.completed_tasks}/${d.planned_tasks} 任务${d.has_review ? '（已复盘）' : ''}`"
            >
              {{ d.date.slice(8) }}
            </span>
          </div>
        </div>
        <p v-else class="prev-report-empty">暂无上周数据（首次使用或上周未生成计划）</p>
      </section>

      <!-- 任务量调整 -->
      <section class="config-section">
        <h4 class="config-section-title">
          <Sparkles :size="14" />
          本周任务量调整（相对上周）
        </h4>
        <div class="wl-direction-grid">
          <button
            v-for="opt in (['increase', 'unchanged', 'decrease'] as WorkloadDirection[])"
            :key="opt"
            type="button"
            class="wl-direction-btn"
            :class="{ active: wlDirection === opt }"
            @click="wlDirection = opt"
          >
            {{ wlDirectionLabel(opt) }}
          </button>
        </div>
        <div v-if="wlDirection !== 'unchanged'" class="wl-level-row">
          <span class="form-label">幅度：</span>
          <button
            v-for="opt in (['small', 'large'] as WorkloadLevel[])"
            :key="opt"
            type="button"
            class="wl-level-btn"
            :class="{ active: wlLevel === opt }"
            @click="wlLevel = opt"
          >
            {{ opt === "small" ? "小幅（约 20%）" : "大幅（约 40%）" }}
          </button>
        </div>
        <input
          v-model="wlNote"
          type="text"
          class="form-input"
          placeholder="备注（可选）：如上周太累、本周状态好…"
          maxlength="100"
        />
      </section>

      <!-- 排除日期 -->
      <section class="config-section">
        <h4 class="config-section-title">
          <CalendarDays :size="14" />
          特殊情况排除日期（本周不学习的日子）
        </h4>
        <p class="config-hint">勾选本周不学习的日期，AI 会把任务量分摊到其他学习日。排除日自动免复盘。</p>
        <div class="exclude-day-grid">
          <div
            v-for="day in configExcludeCandidates"
            :key="day.date"
            class="exclude-day-item"
            :class="{ active: !!configExcluded[day.date] }"
          >
            <label class="exclude-day-toggle">
              <Checkbox
                :checked="!!configExcluded[day.date]"
                @change="toggleConfigExclude(day.date)"
              />
              <span class="exclude-day-label">
                <span class="exclude-day-date">{{ day.date.slice(5) }}</span>
                <span class="exclude-day-weekday">{{ day.weekday }}</span>
                <span v-if="day.isToday" class="exclude-day-today">今天</span>
              </span>
            </label>
            <template v-if="configExcluded[day.date]">
              <Select
                v-model="configExcluded[day.date].reason_type"
                class="exclude-reason-select select-autowidth"
              >
                <option value="travel">外出旅行</option>
                <option value="sick">生病</option>
                <option value="exam">考试</option>
                <option value="other">其他</option>
              </Select>
              <input
                v-model="configExcluded[day.date].note"
                type="text"
                class="form-input"
                placeholder="备注（可选）"
                maxlength="100"
              />
            </template>
          </div>
        </div>
      </section>

      <p v-if="generateError" class="config-error">生成失败：{{ generateError }}</p>
    </div>

    <template #footer>
      <Button variant="ghost" :disabled="generating" @click="emit('close')">取消</Button>
      <Button variant="primary" :loading="generating" :disabled="generating" @click="confirmGenerate">
        <Sparkles :size="14" />
        {{ generating ? "生成中…" : "生成周计划" }}
      </Button>
    </template>
  </Modal>
</template>

<style scoped>
.config-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.config-section {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.config-section-title {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  margin: 0;
}

.config-section-title svg {
  color: var(--accent);
}

.config-hint {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  margin: 0;
}

.config-error {
  margin: 0;
  font-size: var(--text-xs);
  color: var(--color-danger, #ef4444);
}

.form-label {
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
}

.form-input {
  width: 100%;
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: var(--text-sm);
  font-family: inherit;
  transition: border-color var(--transition-fast);
}

.form-input:focus {
  outline: none;
  border-color: var(--accent);
}

.form-input::placeholder {
  color: var(--text-quaternary);
}

/* ── 上周报告 ── */
.prev-report {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.prev-report-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--space-2);
}

.prev-stat {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  padding: var(--space-2);
  background: var(--bg-secondary);
  border-radius: var(--radius-sm);
}

.prev-stat-value {
  font-size: var(--text-base);
  font-weight: var(--font-bold);
  color: var(--text-primary);
}

.prev-stat-value.success {
  color: var(--color-success, #10b981);
}

.prev-stat-value.warning {
  color: var(--color-warning, #f59e0b);
}

.prev-stat-value.danger {
  color: var(--color-danger, #ef4444);
}

.prev-stat-label {
  font-size: 10px;
  color: var(--text-tertiary);
}

.prev-report-days {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
}

.prev-day-chip {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 28px;
  height: 24px;
  padding: 0 6px;
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  background: var(--bg-tertiary);
  color: var(--text-quaternary);
}

.prev-day-chip.studied {
  background: var(--accent-subtle);
  color: var(--accent);
}

.prev-day-chip.done {
  background: var(--color-success, #10b981);
  color: #fff;
}

.prev-day-chip.rest {
  opacity: 0.5;
}

.prev-report-empty {
  font-size: var(--text-xs);
  color: var(--text-quaternary);
  margin: 0;
  text-align: center;
  padding: var(--space-3) 0;
}

/* ── 任务量调整 ── */
.wl-direction-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: var(--space-2);
}

.wl-direction-btn {
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
  color: var(--text-secondary);
  font-size: var(--text-sm);
  font-family: inherit;
  cursor: pointer;
  transition: all var(--transition-fast);
  text-align: center;
}

.wl-direction-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
}

.wl-direction-btn.active {
  border-color: var(--accent);
  background: var(--accent-subtle);
  color: var(--accent);
  font-weight: var(--font-semibold);
}

.wl-level-row {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
}

.wl-level-btn {
  padding: var(--space-1) var(--space-3);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-family: inherit;
  cursor: pointer;
  transition: all var(--transition-fast);
}

.wl-level-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
}

.wl-level-btn.active {
  border-color: var(--accent);
  background: var(--accent-subtle);
  color: var(--accent);
  font-weight: var(--font-semibold);
}

/* ── 排除日 ── */
.exclude-day-grid {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.exclude-day-item {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  transition: border-color var(--transition-fast);
}

.exclude-day-item.active {
  border-color: var(--color-danger, #ef4444);
  background: var(--bg-secondary);
}

.exclude-day-toggle {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  cursor: pointer;
}

.exclude-day-label {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  color: var(--text-primary);
}

.exclude-day-date {
  font-weight: var(--font-medium);
}

.exclude-day-weekday {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.exclude-day-today {
  font-size: 9px;
  color: var(--accent);
  background: var(--accent-subtle);
  padding: 1px 5px;
  border-radius: var(--radius-full);
  font-weight: var(--font-semibold);
}

.exclude-reason-select {
  width: auto;
  min-width: 96px;
}
</style>
