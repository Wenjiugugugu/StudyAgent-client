<script setup lang="ts">
import { computed, ref, watch } from "vue";
import * as api from "@/api";
import { Button, Card } from "@/components/ui";
import ActualStudyEditor from "./ActualStudyEditor.vue";
import { actualStudyError } from "@/utils/actualStudy";
import type { ActualStudyEntry, CurrentStudyDirection } from "@/types";

const props = defineProps<{ refreshKey: string; disabled?: boolean }>();
const emit = defineEmits<{ changed: [] }>();
const directions = ref<CurrentStudyDirection[]>([]);
const draft = ref<ActualStudyEntry[]>([]);
const editing = ref<CurrentStudyDirection | null>(null);
const busy = ref(false);
const error = ref("");
const notice = ref("");
const labels = { math: "数学", english: "英语", politics: "政治", professional: "专业课" };
const invalid = computed(() => actualStudyError(draft.value));
let loadId = 0;
async function load() {
  const id = ++loadId;
  error.value = "";
  editing.value = null;
  draft.value = [];
  try {
    const value = await api.getStudyDirections();
    if (id === loadId) directions.value = value;
  } catch (e) {
    if (id === loadId) error.value = String(e);
  }
}
watch(() => props.refreshKey, load, { immediate: true });
function edit(direction: CurrentStudyDirection) {
  editing.value = direction;
  notice.value = "";
  draft.value = [
    {
      subject: direction.subject,
      content: direction.content,
      scope: "ongoing",
      knowledge_points: [...direction.knowledge_points],
    },
  ];
}
async function save(direction: CurrentStudyDirection, end = false) {
  if (!end && invalid.value) return;
  busy.value = true;
  error.value = "";
  try {
    directions.value = await api.updateStudyDirection(
      end ? { subject: direction.subject, content: "", scope: "end" } : draft.value[0],
      direction.updated_at
    );
    editing.value = null;
    draft.value = [];
    notice.value = end
      ? "已结束这个方向。现有计划保持原样，后续生成计划时生效。"
      : "学习方向已保存。现有计划保持原样，调整预览确认后才会改写计划。";
    emit("changed");
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <Card v-if="directions.length || error || notice" padding="lg" class="direction-panel">
    <h2>当前学习方向</h2>
    <p class="direction-hint">持续有效，无需每天重填。修改或结束方向会保留已有学习记录。</p>
    <section v-for="direction in directions" :key="direction.subject" class="direction-row">
      <template v-if="editing?.subject !== direction.subject">
        <strong>{{ labels[direction.subject] }}</strong>
        <p class="direction-content">{{ direction.content }}</p>
        <p v-if="direction.knowledge_points.length" class="direction-hint">
          关联：{{ direction.knowledge_points.map((p) => p.title).join("、") }}
        </p>
        <div class="direction-actions">
          <Button variant="ghost" size="sm" :disabled="busy || disabled" @click="edit(direction)"
            >修改</Button
          >
          <Button
            variant="ghost"
            size="sm"
            :disabled="busy || disabled"
            @click="save(direction, true)"
            >结束方向</Button
          >
        </div>
      </template>
      <template v-else>
        <ActualStudyEditor v-model="draft" direction-only :disabled="busy || disabled" />
        <div class="direction-actions">
          <Button
            variant="primary"
            size="sm"
            :disabled="!!invalid || busy || disabled"
            @click="save(direction)"
            >保存方向</Button
          >
          <Button variant="ghost" size="sm" :disabled="busy" @click="editing = null">取消</Button>
        </div>
      </template>
    </section>
    <p v-if="error" role="alert" class="direction-error">{{ error }}</p>
    <p v-if="notice" role="status" class="direction-hint">{{ notice }}</p>
  </Card>
</template>

<style scoped>
.direction-panel {
  margin-bottom: 16px;
}
h2 {
  margin: 0 0 8px;
  font-size: 17px;
}
.direction-row {
  padding-top: 14px;
  margin-top: 14px;
  border-top: 1px solid var(--border-color);
}
.direction-content {
  white-space: pre-wrap;
  line-height: 1.7;
  margin: 8px 0;
}
.direction-hint {
  color: var(--text-secondary);
  font-size: 13px;
  line-height: 1.7;
  margin: 0;
}
.direction-actions {
  display: flex;
  gap: 8px;
  margin-top: 8px;
}
.direction-error {
  color: var(--color-danger, #dc2626);
}
</style>
