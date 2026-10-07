<script setup lang="ts">
import { computed, ref, watch } from "vue";
import * as api from "@/api";
import Modal from "@/components/ui/Modal.vue";
import Button from "@/components/ui/Button.vue";
import Checkbox from "@/components/ui/Checkbox.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: []; applied: [] }>();
const preview = ref<api.EnergyPreview | null>(null);
const keep = ref<Record<string, boolean>>({});
const loading = ref(false);
const saving = ref(false);
const error = ref("");
const deferred = computed(
  () => preview.value?.plan.data.tasks.filter((t) => !keep.value[t.id]) ?? []
);
const hours = computed(() => deferred.value.reduce((total, t) => total + t.estimated_hours, 0));
const affectedGoals = computed(
  () =>
    preview.value?.goals.filter(
      (g) => g.active && deferred.value.some((t) => t.subject === g.subject)
    ) ?? []
);

async function load() {
  loading.value = true;
  error.value = "";
  preview.value = null;
  try {
    preview.value = await api.previewLowEnergy();
    // 默认全部保留，用户亲自选择推迟内容。
    keep.value = Object.fromEntries(preview.value.plan.data.tasks.map((t) => [t.id, true]));
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}

async function apply() {
  if (!preview.value || !deferred.value.length || saving.value) return;
  saving.value = true;
  error.value = "";
  try {
    await api.applyLowEnergy(
      deferred.value.map((t) => t.id),
      preview.value.snapshot
    );
    emit("applied");
  } catch (e) {
    error.value = String(e);
  } finally {
    saving.value = false;
  }
}
watch(
  () => props.open,
  (open) => {
    if (open) void load();
  }
);
</script>

<template>
  <Modal :open="open" title="今天精力不够" :width="600" @close="!saving && emit('close')">
    <LoadingSpinner v-if="loading" label="读取今日计划…" />
    <div v-else class="energy-content">
      <p>勾选今天保留的任务，取消勾选的任务会推迟。已开始、已完成和手动任务保持原样。</p>
      <label v-for="task in preview?.plan.data.tasks ?? []" :key="task.id" class="energy-task">
        <Checkbox
          v-model="keep[task.id]"
          :disabled="saving || preview?.protected_ids.includes(task.id)"
        />
        <span
          >{{ task.title
          }}<small
            >{{
              task.estimated_hours > 0 ? `预计 ${task.estimated_hours} 小时` : "预计用时未记录"
            }}
            · {{ keep[task.id] ? "今天保留" : "推迟" }}</small
          ></span
        >
      </label>
      <div v-if="deferred.length" class="energy-impact">
        <p>
          今天保留 {{ (preview?.plan.data.tasks.length ?? 0) - deferred.length }} 项，推迟
          {{ deferred.length }} 项，减少约 {{ hours.toFixed(1) }} 小时。
        </p>
        <p>
          推迟的完整任务会进入待安排队列，生成后续日计划时继续安排。已生成的后续计划需要重新生成才能接纳这些任务。
        </p>
        <p>目标截止日期保持原样，后续学习负担可能增加，暂时无法保证按期完成。</p>
        <p v-for="goal in affectedGoals" :key="goal.id">
          相关目标：{{ goal.title }}，截止 {{ goal.deadline }}。
        </p>
      </div>
      <p v-if="error" role="alert" class="energy-error">{{ error }}</p>
      <div class="energy-actions">
        <Button variant="ghost" :disabled="saving" @click="emit('close')">保持原计划</Button>
        <Button v-if="error" variant="ghost" :disabled="saving" @click="load">重新读取</Button>
        <Button
          variant="primary"
          :disabled="!deferred.length || loading"
          :loading="saving"
          @click="apply"
          >确认保留与推迟</Button
        >
      </div>
    </div>
  </Modal>
</template>

<style scoped>
.energy-content {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}
.energy-content p {
  margin: 0;
  color: var(--text-secondary);
  line-height: 1.6;
}
.energy-task {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) 0;
}
.energy-task small {
  display: block;
  color: var(--text-tertiary);
  margin-top: 4px;
}
.energy-impact {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  border-top: 1px solid var(--divider-color);
  padding-top: var(--space-4);
}
.energy-content .energy-error {
  color: var(--color-danger);
}
.energy-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: var(--space-2);
}
</style>
