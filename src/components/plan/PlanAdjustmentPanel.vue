<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import * as api from "@/api";
import { Button, Checkbox, Modal } from "@/components/ui";
import type { AdjustmentPreview, AdjustmentStatus, AdjustmentOutcome } from "@/types";

const emit = defineEmits<{ changed: []; discarded: [] }>();
const status = ref<AdjustmentStatus>({
  preview: null,
  undo_id: null,
  undo_created_at: null,
  applied_preview: null,
  warnings: [],
  sync_pending: false,
});
const displayPreview = computed(() => status.value.preview ?? status.value.applied_preview);
const busy = ref(false);
const generating = ref(false);
const error = ref("");
const message = ref("");
const syncWarnings = ref<string[]>([]);
const showUndo = ref(false);

async function refresh() {
  try {
    status.value = await api.getPlanAdjustment();
    syncWarnings.value = status.value.warnings;
  } catch (e) {
    error.value = String(e);
  }
}
function showPreview(preview: AdjustmentPreview) {
  status.value.preview = preview;
  error.value = "";
  message.value = "";
}
async function toggle(key: string, checked: boolean) {
  const preview = status.value.preview;
  if (!preview || busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    const keys = checked ? [...preview.kept_keys, key] : preview.kept_keys.filter((k) => k !== key);
    status.value.preview = await api.updatePlanAdjustmentKept(preview.id, keys);
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
async function regenerate() {
  const old = status.value.preview;
  if (!old || busy.value) return;
  busy.value = true;
  generating.value = true;
  error.value = "";
  try {
    let preview =
      old.kind === "review"
        ? await api.regenerateRemainingDays(old.source_date)
        : await api.addExcludedDayAndRegenerate(old.source_date, old.excluded_day!);
    const keys = [
      ...new Set([
        ...preview.kept_keys,
        ...old.kept_keys.filter((k) => preview.tasks.some((t) => t.key === k)),
      ]),
    ];
    preview = await api.updatePlanAdjustmentKept(preview.id, keys);
    showPreview(preview);
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
    generating.value = false;
  }
}
async function cancelGeneration() {
  try {
    await api.cancelAiRequest(api.AI_CANCEL_KEYS.planner);
  } catch (e) {
    error.value = String(e);
  }
}
function recordOutcome(result: AdjustmentOutcome, text: string) {
  message.value = text;
  syncWarnings.value = result.warnings;
  emit("changed");
}
async function apply() {
  const preview = status.value.preview;
  if (!preview || busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    const result = await api.applyPlanAdjustment(preview.id);
    recordOutcome(result, "调整已应用，学习进度和复盘记录继续保留。");
    await refresh();
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
async function discard() {
  if (!status.value.preview || busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    await api.discardPlanAdjustment(status.value.preview.id);
    status.value.preview = null;
    message.value = "已放弃这份预览，现有计划未被改动。";
    emit("discarded");
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
async function undo() {
  if (!status.value.undo_id || busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    const result = await api.undoPlanAdjustment(status.value.undo_id);
    recordOutcome(result, "已撤销最近一次调整，真实学习进度和复盘记录继续保留。");
    showUndo.value = false;
    await refresh();
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
async function retrySync() {
  busy.value = true;
  error.value = "";
  try {
    const result = await api.retryPlanAdjustmentSync();
    syncWarnings.value = result.warnings;
    if (!result.warnings.length) message.value = "滴答同步已完成。";
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
function minutes(hours: number | null) {
  return hours === null ? "无任务" : `${Math.round(hours * 60)} 分钟`;
}
defineExpose({ refresh, showPreview });
onMounted(refresh);
</script>

<template>
  <section
    v-if="
      status.preview ||
      status.undo_id ||
      status.sync_pending ||
      error ||
      message ||
      syncWarnings.length
    "
    class="adjustment-panel"
    aria-label="计划调整"
    :aria-busy="busy"
  >
    <template v-if="displayPreview">
      <div class="adjustment-heading">
        <h2>{{ status.preview ? "调整预览" : "最近一次调整" }}</h2>
        <span>{{ status.preview ? "尚未应用" : "已应用" }}</span>
      </div>
      <p>{{ displayPreview.reason }}</p>
      <p v-if="status.preview" class="muted">
        确认后才会保存计划并同步滴答。你可以先指定要保留的任务，也可以放弃这份预览。
      </p>
      <p v-if="generating" role="status">正在重新生成预览，现有计划保持不变…</p>
      <p v-if="displayPreview.used_fallback" class="notice">
        {{
          status.preview
            ? "AI 未能完成调整，下面是按原任务生成的备用方案；确认后才会应用。"
            : "本次使用了按原任务生成的备用方案。"
        }}
      </p>
      <p v-for="warning in displayPreview.warnings" :key="warning" class="notice">{{ warning }}</p>
      <div v-if="displayPreview.conflicts.length" role="alert">
        <p v-for="conflict in displayPreview.conflicts" :key="conflict" class="error">
          {{ conflict }}
        </p>
      </div>
      <details v-if="status.preview && displayPreview.tasks.length" class="keep-tasks">
        <summary>保留任务 · 已选择 {{ displayPreview.kept_keys.length }} 项</summary>
        <p class="muted">
          已完成、正在学习和手动任务自动保留。其他任务也可勾选，日期、内容和预计时长保持不变。
        </p>
        <label v-for="choice in displayPreview.tasks" :key="choice.key" class="keep-task">
          <Checkbox
            :checked="displayPreview.kept_keys.includes(choice.key)"
            :disabled="busy || choice.required"
            @change="toggle(choice.key, $event)"
          />
          <span
            >{{ choice.date }} · {{ choice.task.title
            }}<small
              >{{ minutes(choice.task.estimated_hours)
              }}{{ choice.required ? " · 自动保留" : "" }}</small
            ></span
          >
        </label>
      </details>
      <div class="day-changes">
        <article v-for="day in displayPreview.days" :key="day.date" class="day-change">
          <h3>
            {{ day.date }}
            <span>{{ minutes(day.before_hours) }} → {{ minutes(day.after_hours) }}</span>
          </h3>
          <p v-if="day.before_rest !== day.after_rest" class="notice">
            {{ day.after_rest ? "调整为休息日" : "调整为学习日" }}
          </p>
          <div
            v-for="(change, index) in displayPreview.changes.filter(
              (c) =>
                c.before_date === day.date || (c.before_date === null && c.after_date === day.date)
            )"
            :key="index"
            class="task-change"
          >
            <strong>{{ change.subject }} · {{ change.title }}</strong>
            <p>
              {{ change.before_date || "新增" }}（{{ minutes(change.before_hours) }}） →
              {{ change.after_date || "移出本次计划" }}（{{ minutes(change.after_hours) }}）
            </p>
            <small>{{ change.reason }}</small>
            <p
              v-if="
                change.before_content &&
                change.after_content &&
                change.before_content !== change.after_content
              "
              class="muted"
            >
              学习要求：{{ change.before_content }} → {{ change.after_content }}
            </p>
          </div>
          <p
            v-if="
              !displayPreview.changes.some(
                (c) => c.before_date === day.date || c.after_date === day.date
              )
            "
            class="muted"
          >
            任务安排保持不变。
          </p>
          <p
            v-else-if="
              displayPreview.changes.some(
                (c) => c.before_date && c.before_date !== day.date && c.after_date === day.date
              )
            "
            class="muted"
          >
            有任务移入此日，原日期下展示了移动详情。
          </p>
        </article>
      </div>
      <article v-for="(goal, index) in displayPreview.goals" :key="index" class="day-change">
        <h3>{{ goal.title || "目标安排" }}</h3>
        <p>截止日：{{ goal.before_deadline }} → {{ goal.after_deadline }}</p>
        <p>后续安排起点：{{ goal.before_start || "原计划起点" }} → {{ goal.after_start }}</p>
        <small>{{ displayPreview.reason }}</small>
      </article>
      <p v-if="!displayPreview.affected_dates.length" class="muted">本次没有需要调整的后续日期。</p>
      <div v-if="status.preview" class="actions">
        <Button
          variant="primary"
          :loading="busy"
          :disabled="
            busy ||
            !!displayPreview.conflicts.length ||
            (!displayPreview.affected_dates.length && !displayPreview.goals.length)
          "
          @click="apply"
          >应用调整</Button
        >
        <Button variant="ghost" :disabled="busy" @click="discard">放弃预览</Button>
        <Button v-if="!generating" variant="ghost" :disabled="busy" @click="regenerate"
          >重新生成预览</Button
        >
        <Button v-else variant="ghost" @click="cancelGeneration">取消生成</Button>
      </div>
    </template>
    <p v-if="message" role="status">{{ message }}</p>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <p v-for="warning in syncWarnings" :key="warning" class="notice">{{ warning }}</p>
    <div class="actions">
      <Button v-if="status.undo_id" variant="ghost" :disabled="busy" @click="showUndo = true"
        >撤销最近一次调整</Button
      >
      <Button v-if="status.sync_pending" variant="secondary" :disabled="busy" @click="retrySync"
        >重试同步滴答</Button
      >
    </div>
    <Modal
      :open="showUndo"
      title="撤销最近一次调整"
      :close-on-overlay="!busy"
      :close-on-esc="!busy"
      :show-close="!busy"
      @close="showUndo = false"
    >
      <p>恢复 {{ status.undo_created_at }} 这次调整之前的计划和目标安排，并同步滴答。</p>
      <p>真实学习进度、计时和复盘继续保留。已经开始或完成的新增任务也会保留；过去日期不再改动。</p>
      <template #footer
        ><Button variant="ghost" :disabled="busy" @click="showUndo = false">取消</Button
        ><Button variant="primary" :loading="busy" :disabled="busy" @click="undo"
          >确认撤销</Button
        ></template
      >
    </Modal>
  </section>
</template>

<style scoped>
.adjustment-panel {
  padding: var(--space-5);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-lg);
  color: var(--text-primary);
  background: var(--bg-secondary);
  text-align: left;
}
.adjustment-panel p {
  margin: var(--space-2) 0;
  line-height: 1.6;
}
.adjustment-heading,
.actions {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  flex-wrap: wrap;
}
.adjustment-heading h2 {
  font-size: var(--text-lg);
  margin: 0;
}
.adjustment-heading > span,
.muted,
small,
h3 span {
  color: var(--text-secondary);
  font-weight: normal;
}
.notice {
  color: var(--color-warning);
}
.error {
  color: var(--color-danger);
}
.keep-tasks {
  margin: var(--space-4) 0;
}
summary {
  cursor: pointer;
  padding: var(--space-2) 0;
}
.keep-task {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) 0;
}
.keep-task small {
  display: block;
  margin-top: var(--space-1);
}
.day-change {
  border-top: 1px solid var(--border-color);
  padding: var(--space-3) 0;
}
.day-change h3 {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-3);
  font-size: var(--text-sm);
  margin: var(--space-2) 0;
}
.task-change {
  padding: var(--space-2) 0;
  overflow-wrap: anywhere;
}
.task-change strong {
  font-size: var(--text-sm);
}
.task-change p {
  margin: var(--space-1) 0;
}
.actions {
  margin-top: var(--space-3);
}
</style>
