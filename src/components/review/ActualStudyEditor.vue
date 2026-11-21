<script setup lang="ts">
import { computed, ref } from "vue";
import { Button, Select } from "@/components/ui";
import KnowledgePointPicker from "@/components/plan/KnowledgePointPicker.vue";
import type { ActualStudyEntry, SubjectKey } from "@/types";
import { actualStudyError } from "@/utils/actualStudy";

const props = defineProps<{
  modelValue: ActualStudyEntry[];
  directionOnly?: boolean;
  disabled?: boolean;
}>();
const emit = defineEmits<{ "update:modelValue": [entries: ActualStudyEntry[]] }>();
const subjects: { key: SubjectKey; label: string }[] = [
  { key: "math", label: "数学" },
  { key: "english", label: "英语" },
  { key: "politics", label: "政治" },
  { key: "professional", label: "专业课" },
];
const error = computed(() => actualStudyError(props.modelValue));
const noticeSubject = ref<string | null>(null);
function selectScope(index: number, scope: "ongoing" | "today") {
  noticeSubject.value = props.modelValue[index].subject;
  patch(index, { scope });
}
function patch(index: number, value: Partial<ActualStudyEntry>) {
  emit(
    "update:modelValue",
    props.modelValue.map((entry, i) => (i === index ? { ...entry, ...value } : entry))
  );
}
function add() {
  const subject = subjects.find((s) => !props.modelValue.some((e) => e.subject === s.key));
  if (subject)
    emit("update:modelValue", [
      ...props.modelValue,
      { subject: subject.key, content: "", scope: "ongoing", knowledge_points: [] },
    ]);
}
</script>

<template>
  <div class="actual-editor">
    <section v-for="(entry, index) in modelValue" :key="index" class="actual-entry">
      <div class="actual-heading">
        <Select
          :model-value="entry.subject"
          :disabled="disabled || directionOnly"
          @update:model-value="
            patch(index, { subject: $event as SubjectKey, knowledge_points: [] })
          "
        >
          <option v-for="subject in subjects" :key="subject.key" :value="subject.key">
            {{ subject.label }}
          </option>
        </Select>
        <Button
          v-if="!directionOnly"
          variant="ghost"
          size="sm"
          :disabled="disabled"
          @click="
            emit(
              'update:modelValue',
              modelValue.filter((_, i) => i !== index)
            )
          "
          >移除</Button
        >
      </div>
      <label class="actual-field">
        <span>我实际正在学什么，接下来想怎么安排</span>
        <textarea
          :value="entry.content"
          :disabled="disabled"
          maxlength="2000"
          rows="3"
          placeholder="例如：我已经开始线代强化，正在学行列式。接下来想先完成线代强化，再继续高数强化。"
          @input="patch(index, { content: ($event.target as HTMLTextAreaElement).value })"
        />
      </label>
      <div v-if="!directionOnly" class="actual-scopes" role="group" aria-label="影响范围">
        <Button
          :variant="entry.scope === 'ongoing' ? 'primary' : 'ghost'"
          size="sm"
          :disabled="disabled"
          :aria-pressed="entry.scope === 'ongoing'"
          @click="selectScope(index, 'ongoing')"
          >{{
            entry.scope === "ongoing" ? "已选择：后续按这个方向安排" : "接下来按这个方向安排"
          }}</Button
        >
        <Button
          :variant="entry.scope === 'today' ? 'primary' : 'ghost'"
          size="sm"
          :disabled="disabled"
          :aria-pressed="entry.scope === 'today'"
          @click="selectScope(index, 'today')"
          >{{ entry.scope === "today" ? "已选择：仅记录当天" : "仅记录当天" }}</Button
        >
      </div>
      <p
        v-if="!directionOnly && noticeSubject === entry.subject"
        class="actual-notice"
        role="status"
      >
        {{
          entry.scope === "ongoing"
            ? "已选择后续按这个方向安排，尚未保存。请点击页面底部的「提交复盘」；提交后 AI 会生成调整预览，你确认应用后才会修改计划。"
            : "已选择仅记录当天，尚未保存。请点击页面底部的「提交复盘」；这条说明不会改变后续学习方向。"
        }}
      </p>
      <p class="actual-hint">
        {{
          directionOnly
            ? "点击「保存方向」后记住这项修改；后续生成计划时使用，已有计划需生成调整预览并确认应用后才会改变。"
            : entry.scope === "today"
              ? "只记录这一天的实际学习，不改变后续方向。"
              : "提交复盘后记住这个方向，并由 AI 生成调整预览；确认应用后才会修改计划。这个方向持续有效，直到你修改或结束。"
        }}
      </p>
      <details v-if="!disabled">
        <summary>
          关联知识点（可选{{
            entry.knowledge_points?.length ? `，已选 ${entry.knowledge_points.length} 个` : ""
          }}）
        </summary>
        <KnowledgePointPicker
          :subject="entry.subject"
          :model-value="entry.knowledge_points ?? []"
          @update:model-value="patch(index, { knowledge_points: $event })"
        />
      </details>
    </section>
    <p v-if="error" class="actual-error" role="alert">{{ error }}</p>
    <Button
      v-if="!directionOnly && modelValue.length < 4"
      variant="ghost"
      size="sm"
      :disabled="disabled"
      @click="add"
      >{{ modelValue.length ? "补充其他科目" : "实际学习内容与计划不同？告诉 AI" }}</Button
    >
    <p v-if="modelValue.length" class="actual-hint">
      “正在学”不会自动算作已完成，也不会修改知识点掌握程度。
    </p>
  </div>
</template>

<style scoped>
.actual-editor,
.actual-entry {
  display: grid;
  gap: 12px;
}
.actual-entry {
  padding: 16px;
  border: 1px solid var(--border-color);
  border-radius: 12px;
}
.actual-heading,
.actual-scopes {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}
.actual-heading {
  justify-content: space-between;
}
.actual-field {
  display: grid;
  gap: 8px;
  font-size: 14px;
}
textarea {
  width: 100%;
  box-sizing: border-box;
  resize: vertical;
  padding: 12px;
  border: 1px solid var(--border-color-strong);
  border-radius: 8px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  font: inherit;
  line-height: 1.7;
}
.actual-hint {
  margin: 0;
  color: var(--text-secondary);
  font-size: 13px;
  line-height: 1.7;
}
.actual-error {
  margin: 0;
  color: var(--color-danger, #dc2626);
}
.actual-notice {
  margin: 0;
  padding: 10px 12px;
  border-left: 3px solid var(--accent);
  border-radius: 6px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  font-size: 13px;
  line-height: 1.7;
}
summary {
  cursor: pointer;
  font-size: 13px;
  color: var(--text-secondary);
  padding-bottom: 8px;
}
</style>
