<script setup>
import { ref } from 'vue';
import ActualStudyEditor from '/src/components/review/ActualStudyEditor.vue';
import StudyDirectionPanel from '/src/components/review/StudyDirectionPanel.vue';
const entries = ref([]);
let directions = [{subject:'math', content:'先完成线代强化，再继续高数强化。当前正在学行列式。', knowledge_points:[], source_date:'2026-10-07', updated_at:'v1'}];
window.qaCalls = [];
window.__TAURI_INTERNALS__ = { invoke: async (command, args) => {
  args = args ? JSON.parse(JSON.stringify(args)) : args;
  window.qaCalls.push({command, args});
  if(command === 'get_study_directions') return structuredClone(directions);
  if(command === 'list_progress_tables') return {subjects:{}};
  if(command === 'update_study_direction') {
    if(args.expectedUpdatedAt !== directions[0].updated_at) throw new Error('学习方向已发生变化');
    directions = args.entry.scope === 'end' ? [] : [{...args.entry, knowledge_points:args.entry.knowledge_points ?? [], source_date:'2026-10-07', updated_at:'v2'}];
    return structuredClone(directions);
  }
  throw new Error('未提供测试命令 '+command);
}};
</script>
<template>
  <main style="max-width:780px;margin:30px auto;padding:20px">
    <StudyDirectionPanel refresh-key="qa" />
    <section style="padding:24px;border:1px solid var(--border-color);border-radius:16px">
      <h2 style="margin:0 0 12px">我实际正在学什么（可选）</h2>
      <p style="color:var(--text-secondary);line-height:1.7">实际安排与 AI 计划差得较多时，在这里说明当前内容和接下来的顺序。只填文字也能生效。</p>
      <ActualStudyEditor v-model="entries" />
      <pre id="entries" style="display:none">{{ JSON.stringify(entries) }}</pre>
    </section>
  </main>
</template>
