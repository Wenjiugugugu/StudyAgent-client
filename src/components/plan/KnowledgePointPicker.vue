<script setup lang="ts">
import { computed, ref, watch } from "vue";
import * as api from "@/api";
import Checkbox from "@/components/ui/Checkbox.vue";
import type { KnowledgePointRef, ProgressIndex } from "@/types";

const props = defineProps<{ subject: string; modelValue: KnowledgePointRef[] }>();
const emit = defineEmits<{ "update:modelValue": [points: KnowledgePointRef[]] }>();
const index = ref<ProgressIndex | null>(null);
const search = ref("");
const error = ref("");
const loading = ref(false);
watch(
  () => props.subject,
  async () => {
    loading.value = true;
    error.value = "";
    search.value = "";
    try {
      index.value = await api.listProgressTables();
    } catch (e) {
      error.value = String(e);
    } finally {
      loading.value = false;
    }
  },
  { immediate: true }
);
const groups = computed(() => {
  const set = index.value?.subjects[props.subject];
  return (set?.tables ?? [])
    .filter((t) => !set?.active_variant || t.variant === set.active_variant)
    .map((table) => ({
      table,
      nodes: table.nodes.filter(
        (n) => n.level === "knowledge" && `${n.phase} ${n.title}`.includes(search.value.trim())
      ),
    }))
    .filter((g) => g.nodes.length);
});
function selected(tableId: string, nodeId: string) {
  return props.modelValue.some((p) => p.table_id === tableId && p.node_id === nodeId);
}
function toggle(point: KnowledgePointRef) {
  emit(
    "update:modelValue",
    selected(point.table_id, point.node_id)
      ? props.modelValue.filter((p) => p.table_id !== point.table_id || p.node_id !== point.node_id)
      : [...props.modelValue, point]
  );
}
</script>

<template>
  <div class="point-picker">
    <input
      v-model="search"
      class="point-search"
      placeholder="搜索知识点或章节"
      aria-label="搜索知识点或章节"
    />
    <p class="point-summary">已关联 {{ modelValue.length }} 个知识点</p>
    <div v-if="modelValue.length" class="point-selected">
      <button
        v-for="point in modelValue"
        :key="`${point.table_id}/${point.node_id}`"
        type="button"
        @click="toggle(point)"
        :aria-label="`取消关联 ${point.title}`"
      >
        {{ point.title }} ×
      </button>
    </div>
    <p v-if="loading">正在加载知识点…</p>
    <p v-else-if="error" role="alert">{{ error }}</p>
    <div v-else class="point-options">
      <p v-if="!groups.length">没有可选知识点。你可以先在进度页创建进度表。</p>
      <section v-for="group in groups" :key="group.table.id">
        <h4>{{ group.table.name }}</h4>
        <label v-for="node in group.nodes" :key="node.id" class="point-option">
          <Checkbox
            :checked="selected(group.table.id, node.id)"
            @change="toggle({ table_id: group.table.id, node_id: node.id, title: node.title })"
          />
          <span
            >{{ node.title }}<small>{{ node.phase }}</small></span
          >
        </label>
      </section>
    </div>
  </div>
</template>

<style scoped>
.point-picker {
  display: grid;
  gap: 10px;
}
.point-search {
  width: 100%;
  padding: 9px 12px;
  border: 1px solid var(--border-color-strong);
  border-radius: 8px;
  background: var(--bg-secondary);
  color: var(--text-primary);
}
.point-summary {
  margin: 0;
  font-size: 13px;
  color: var(--text-secondary);
}
.point-options {
  max-height: 280px;
  overflow-y: auto;
}
.point-options h4 {
  margin: 12px 0 8px;
  font-size: 13px;
}
.point-option {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 4px;
  cursor: pointer;
}
.point-option small {
  display: block;
  margin-top: 3px;
  color: var(--text-secondary);
  font-size: 11px;
}
.point-selected {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.point-selected button {
  border: 1px solid var(--border-color-strong);
  border-radius: 6px;
  padding: 5px 8px;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
}
</style>
