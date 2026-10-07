<script setup lang="ts">
import { ref } from "vue";
import * as api from "@/api";
import { Button, DatePicker, Modal, Select } from "@/components/ui";
import { todayString, getWeekStart } from "@/utils/date";
import type { AdjustmentPreview, ExcludedReasonType } from "@/types";

const emit = defineEmits<{ preview: [AdjustmentPreview] }>();
const open = ref(false);
const date = ref(todayString());
const reason = ref<ExcludedReasonType>("other");
const note = ref("");
const busy = ref(false);
const error = ref("");
const reasons = [
  { value: "travel", label: "出行" },
  { value: "sick", label: "生病" },
  { value: "exam", label: "考试" },
  { value: "other", label: "其他" },
];
async function generate() {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    const preview = await api.addExcludedDayAndRegenerate(getWeekStart(date.value), {
      date: date.value,
      reason_type: reason.value,
      note: note.value.trim() || undefined,
    });
    emit("preview", preview);
    open.value = false;
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
async function cancel() {
  try {
    await api.cancelAiRequest(api.AI_CANCEL_KEYS.planner);
  } catch (e) {
    error.value = String(e);
  }
}
</script>

<template>
  <Button variant="ghost" size="sm" @click="open = true">新增不学习日期</Button>
  <Modal
    :open="open"
    title="新增不学习日期"
    :close-on-overlay="!busy"
    :close-on-esc="!busy"
    :show-close="!busy"
    @close="open = false"
  >
    <div class="excluded-fields">
      <p>选择今天或之后的一天，先查看任务如何调整，确认后才生效。该日期所在周需要已有周计划。</p>
      <label>日期<DatePicker v-model="date" :min="todayString()" :disabled="busy" /></label>
      <label
        >原因<Select v-model="reason" :disabled="busy"
          ><option v-for="item in reasons" :key="item.value" :value="item.value">
            {{ item.label }}
          </option></Select
        ></label
      >
      <label
        >备注<input v-model="note" class="form-input" placeholder="可不填" :disabled="busy"
      /></label>
      <p v-if="busy" role="status">正在生成预览，现有计划保持不变…</p>
      <p v-if="error" class="error" role="alert">{{ error }}</p>
    </div>
    <template #footer>
      <Button v-if="busy" variant="ghost" @click="cancel">取消生成</Button>
      <Button v-else variant="ghost" @click="open = false">取消</Button>
      <Button
        variant="primary"
        :loading="busy"
        :disabled="busy || !date || date < todayString()"
        @click="generate"
        >生成调整预览</Button
      >
    </template>
  </Modal>
</template>

<style scoped>
.excluded-fields {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}
.excluded-fields p {
  line-height: 1.6;
  margin: 0;
}
label {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}
.error {
  color: var(--color-danger);
}
</style>
