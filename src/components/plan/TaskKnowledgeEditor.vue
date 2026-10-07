<script setup lang="ts">
import { ref, watch } from "vue";
import * as api from "@/api";
import Modal from "@/components/ui/Modal.vue";
import Button from "@/components/ui/Button.vue";
import KnowledgePointPicker from "./KnowledgePointPicker.vue";
import type { DailyPlan, KnowledgePointRef, PlanTask } from "@/types";
const props = defineProps<{ task: PlanTask | null }>();
const emit = defineEmits<{ close: []; saved: [plan: DailyPlan] }>();
const points = ref<KnowledgePointRef[]>([]);
const saving = ref(false);
const error = ref("");
watch(
  () => props.task,
  (task) => {
    points.value = [...(task?.knowledge_points ?? [])];
    error.value = "";
  },
  { immediate: true }
);
async function save() {
  if (!props.task || saving.value) return;
  saving.value = true;
  try {
    emit("saved", await api.setTaskKnowledgePoints(props.task.id, points.value));
    emit("close");
  } catch (e) {
    error.value = String(e);
  } finally {
    saving.value = false;
  }
}
</script>
<template>
  <Modal :open="!!task" title="关联知识点" :width="560" @close="!saving && emit('close')">
    <p>{{ task?.title }}</p>
    <p class="editor-hint">选择这个任务要学的内容。取消所有关联后，这项任务将不自动推进进度。</p>
    <KnowledgePointPicker v-if="task" v-model="points" :subject="task.subject" />
    <p v-if="error" role="alert">{{ error }}</p>
    <template #footer>
      <Button variant="secondary" :disabled="saving" @click="emit('close')">取消</Button>
      <Button variant="primary" :loading="saving" @click="save">保存关联</Button>
    </template>
  </Modal>
</template>
<style scoped>
.editor-hint {
  color: var(--text-secondary);
  font-size: 13px;
  line-height: 1.6;
}
</style>
