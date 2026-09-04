<script setup lang="ts">
/**
 * 「批量更改进度」弹窗
 *
 * 快速调整各科每本进度表的进度：选择「学到第几章」，之前的章节自动全部推进；
 * 未学完的章节点开章节细则逐个勾选已学知识点；顶部选择本轮状态（基础 / 强化）。
 * 专业课内置场景下，总专业课进度表由各教材覆盖度自动推导（本弹窗不列总表供手动编辑）。
 * 提交后走后端 batch_update_progress（只升不降），父组件收到 applied 事件后刷新。
 */
import { computed, ref, reactive, watch } from "vue";
import * as api from "@/api";
import Button from "@/components/ui/Button.vue";
import Badge from "@/components/ui/Badge.vue";
import Modal from "@/components/ui/Modal.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import { BookOpen, CheckCircle2, Circle, CircleDot } from "lucide-vue-next";
import type { ProgressIndex, ProgressNode, ProgressNodeStatus, ProgressTable } from "@/types";

const props = withDefaults(
  defineProps<{
    open: boolean;
    /** 全部科目进度表索引（后台列表数据） */
    index: ProgressIndex | null;
    /** 科目 → 当前启用方案（如 math → 数二） */
    variants: Record<string, string>;
  }>(),
  { open: false, index: null }
);

const emit = defineEmits<{
  (e: "close"): void;
  (e: "applied", result: api.BatchUpdateResult): void;
}>();

const SUBJECTS = [
  { key: "math", label: "数学" },
  { key: "english", label: "英语" },
  { key: "politics", label: "政治" },
  { key: "professional", label: "专业课" },
];

/** 本轮状态：基础 / 强化 */
const ROUNDS: { value: ProgressNodeStatus; label: string; hint: string }[] = [
  { value: "basic", label: "基础", hint: "第一轮基础学习" },
  { value: "reinforcing", label: "强化", hint: "第二轮强化提高" },
];

const STATUS_RANK: Record<ProgressNodeStatus, number> = {
  pending: 0,
  learning: 1,
  basic: 2,
  reinforcing: 3,
  mastered: 4,
};

const round = ref<ProgressNodeStatus>("basic");
const applying = ref(false);
const error = ref("");
const resultMsg = ref("");

/** 每张表的选中状态：key = `${subject}:${tableId}` */
interface TableSel {
  /** 学到第几章（chapters 下标）；null = 未选择 */
  reached: number | null;
  /** 当前章是否整章学完 */
  fullCurrent: boolean;
  /** 当前章内已勾选的知识点 id（只对当前章生效） */
  checked: Set<string>;
}
const sels = reactive<Record<string, TableSel>>({});

function selKey(subject: string, tableId: string): string {
  return `${subject}:${tableId}`;
}
function getSel(subject: string, tableId: string): TableSel {
  const k = selKey(subject, tableId);
  if (!sels[k]) sels[k] = { reached: null, fullCurrent: false, checked: new Set() };
  return sels[k];
}
function clearSel(subject: string, tableId: string) {
  const k = selKey(subject, tableId);
  if (sels[k]) delete sels[k];
}
function resetAll() {
  Object.keys(sels).forEach((k) => delete sels[k]);
  error.value = "";
  resultMsg.value = "";
}

watch(
  () => props.open,
  (v) => {
    if (v) resetAll();
  }
);

// ── 科目 / 方案 / 表 ──
function variantOf(subject: string): string {
  return props.variants[subject] ?? "";
}
function tableInVariant(t: ProgressTable, subject: string): boolean {
  const v = variantOf(subject);
  if (t.variant === v) return true;
  if (t.variant === "" && (!v || v === "默认")) return true;
  return false;
}
function tablesOf(subject: string): ProgressTable[] {
  const set = props.index?.subjects[subject];
  if (!set) return [];
  return (set.tables ?? []).filter((t) => tableInVariant(t, subject));
}
const anyTables = computed(() => SUBJECTS.some((s) => tablesOf(s.key).length > 0));

/** 是否为「由教材自动推导」的总专业课进度表（只读，不手动编辑） */
function isDerivedMaster(subject: string, t: ProgressTable): boolean {
  const hasBooks = tablesOf(subject).some((x) => x.name.includes("教材："));
  return hasBooks && t.name.includes("总");
}

interface ChapterItem {
  id: string;
  title: string;
  kids: ProgressNode[];
}
function chaptersOf(t: ProgressTable): ChapterItem[] {
  const chs = t.nodes.filter((n) => n.level === "chapter");
  return chs.map((c) => ({
    id: c.id,
    title: c.title,
    kids: t.nodes.filter((n) => n.level === "knowledge" && n.parent_id === c.id),
  }));
}
function knowledgeCount(t: ProgressTable): number {
  return t.nodes.filter((n) => n.level === "knowledge").length;
}

// ── 覆盖预览 ──
function coveredCount(t: ProgressTable, sel: TableSel): number {
  const chs = chaptersOf(t);
  if (sel.reached == null || !chs[sel.reached]) return 0;
  let n = 0;
  for (let i = 0; i < sel.reached; i++) n += chs[i].kids.length;
  const cur = chs[sel.reached];
  if (cur) {
    if (sel.fullCurrent) n += cur.kids.length;
    else n += cur.kids.filter((k) => sel.checked.has(k.id)).length;
  }
  return n;
}

/** 章节行图标状态：已覆盖 / 正在学（部分覆盖）/ 未学 */
function chapterIcon(subject: string, tableId: string, idx: number) {
  const sel = getSel(subject, tableId);
  if (sel.reached != null && idx < sel.reached) return CheckCircle2;
  if (sel.reached === idx && sel.fullCurrent) return CheckCircle2;
  if (sel.reached === idx) return CircleDot;
  return Circle;
}
function chapterCovered(subject: string, tableId: string, idx: number) {
  const sel = getSel(subject, tableId);
  return (sel.reached != null && idx < sel.reached) || (sel.reached === idx && sel.fullCurrent);
}
function chapterCurrent(subject: string, tableId: string, idx: number) {
  return getSel(subject, tableId).reached === idx;
}

function pickChapter(subject: string, tableId: string, idx: number) {
  const sel = getSel(subject, tableId);
  if (sel.reached === idx) return; // 已选中：重复点击不变化（可用「重置」清空）
  sel.reached = idx;
  sel.fullCurrent = false;
}
function toggleFull(subject: string, tableId: string, idx: number) {
  const sel = getSel(subject, tableId);
  sel.reached = idx;
  sel.fullCurrent = !sel.fullCurrent;
}
function toggleKid(subject: string, tableId: string, kidId: string) {
  const sel = getSel(subject, tableId);
  if (sel.checked.has(kidId)) sel.checked.delete(kidId);
  else sel.checked.add(kidId);
}

// ── 提交 ──
function buildUpdates(): api.BatchSubjectUpdate[] {
  const out: api.BatchSubjectUpdate[] = [];
  for (const s of SUBJECTS) {
    const tables = tablesOf(s.key);
    const list: api.BatchTableCoverage[] = [];
    for (const t of tables) {
      if (isDerivedMaster(s.key, t)) continue; // 总表由后端按教材推导
      const sel = sels[selKey(s.key, t.id)];
      if (!sel) continue;
      const chs = chaptersOf(t);
      if (sel.reached == null || !chs[sel.reached]) continue;
      const cur = chs[sel.reached];
      list.push({
        table_id: t.id,
        reached_chapter: cur.id,
        current_full: !!sel.fullCurrent,
        current_points: sel.fullCurrent
          ? []
          : Array.from(sel.checked).filter((id) => cur.kids.some((k) => k.id === id)),
      });
    }
    if (list.length) out.push({ subject: s.key, round: round.value, tables: list });
  }
  return out;
}

async function apply() {
  const updates = buildUpdates();
  if (!updates.length) {
    error.value = "还没有选择任何书籍的进度，先为至少一本书选择「学到第几章」";
    return;
  }
  applying.value = true;
  error.value = "";
  resultMsg.value = "";
  try {
    const res = await api.batchUpdateProgress(updates);
    resultMsg.value = `已更新 ${res.tables_updated} 张表，推进 ${res.nodes_changed} 个节点`;
    emit("applied", res);
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    applying.value = false;
  }
}

function emitClose() {
  error.value = "";
  resultMsg.value = "";
  emit("close");
}

/** 汇总概览：本轮共有多少张表、多少知识点会被推进 */
const summary = computed(() => {
  let tables = 0;
  let nodes = 0;
  const targetRank = STATUS_RANK[round.value];
  for (const s of SUBJECTS) {
    for (const t of tablesOf(s.key)) {
      if (isDerivedMaster(s.key, t) || !sels[selKey(s.key, t.id)]) continue;
      const sel = sels[selKey(s.key, t.id)];
      if (sel.reached == null) continue;
      const chs = chaptersOf(t);
      let cov = 0;
      for (let i = 0; i < sel.reached; i++) cov += chs[i].kids.length;
      if (chs[sel.reached]) {
        cov += sel.fullCurrent
          ? chs[sel.reached].kids.length
          : chs[sel.reached].kids.filter((k) => sel.checked.has(k.id)).length;
      }
      const alreadyAdvanced = t.nodes.filter(
        (n) =>
          n.level === "knowledge" &&
          STATUS_RANK[n.status as ProgressNodeStatus] >= targetRank
      ).length;
      const toAdvance = Math.max(0, cov - alreadyAdvanced);
      if (toAdvance > 0) {
        tables += 1;
        nodes += toAdvance;
      }
    }
  }
  return { tables, nodes };
});
</script>

<template>
  <Modal
    :open="open"
    title="批量更改进度"
    :width="640"
    :close-on-overlay="false"
    @close="emitClose"
  >
    <div class="batch-body">
      <p class="form-hint">
        选择每本书「学到第几章」，之前的章节将自动全部推进（只升不降，不会降低已掌握的知识点）；
        未学完的章节点开章节细则，勾选已学到的知识点。
      </p>

      <!-- 本轮状态 -->
      <div class="round-bar">
        <span class="bar-label">本轮状态</span>
        <div class="seg">
          <button
            v-for="r in ROUNDS"
            :key="r.value"
            type="button"
            class="seg-btn"
            :class="{ active: round === r.value }"
            @click="round = r.value"
          >
            {{ r.label }}
          </button>
        </div>
        <span class="bar-hint">{{ ROUNDS.find((r) => r.value === round)?.hint }}</span>
      </div>
      <p v-if="summary.tables" class="summary-line">
        本轮将把 {{ summary.tables }} 张表中的 {{ summary.nodes }} 个知识点推进为「{{
          ROUNDS.find((r) => r.value === round)?.label
        }}」。
      </p>

      <div v-if="error" class="error-banner" role="alert">{{ error }}</div>
      <p v-if="resultMsg" class="success-msg">{{ resultMsg }}</p>

      <EmptyState
        v-if="!anyTables"
        title="还没有进度表"
        description="请先在进度页为科目加载或创建进度表，再进行批量更新。"
      />

      <div v-for="s in SUBJECTS" :key="s.key" class="subj-block">
        <template v-if="tablesOf(s.key).length">
          <div class="subj-head">
            <Badge variant="info">{{ s.label }}</Badge>
            <span class="subj-variant">{{ variantOf(s.key) }}</span>
          </div>

          <div v-for="t in tablesOf(s.key)" :key="t.id" class="table-block">
            <!-- 总专业课进度表：由教材自动推导，只读 -->
            <div v-if="isDerivedMaster(s.key, t)" class="master-note">
              <Badge variant="success">总表</Badge>
              <span class="master-name">{{ t.name }}</span>
              <span class="master-tip">将根据各教材进度自动更新（无需手动填写）</span>
            </div>

            <template v-else>
              <div class="table-head">
                <BookOpen :size="14" class="book-icon" />
                <span class="table-name">{{ t.name }}</span>
                <Badge :variant="t.origin === 'builtin' ? 'info' : 'default'">
                  {{ t.origin === "builtin" ? "内置" : "自定义" }}
                </Badge>
                <span class="table-cov">
                  已覆盖 {{ coveredCount(t, getSel(s.key, t.id)) }}/{{ knowledgeCount(t) }}
                </span>
                <button
                  type="button"
                  class="reset-btn"
                  title="清除该书的进度选择"
                  @click="clearSel(s.key, t.id)"
                >
                  重置
                </button>
              </div>

              <div class="chapters">
                <div
                  v-for="(ch, idx) in chaptersOf(t)"
                  :key="ch.id"
                  class="chapter-row"
                  :class="{
                    current: chapterCurrent(s.key, t.id, idx),
                    covered: chapterCovered(s.key, t.id, idx),
                  }"
                >
                  <div class="chapter-line">
                    <button type="button" class="chapter-main" @click="pickChapter(s.key, t.id, idx)">
                      <component :is="chapterIcon(s.key, t.id, idx)" :size="14" class="ch-ic" />
                      <span class="ch-title" :title="ch.title">{{ ch.title }}</span>
                      <span class="ch-count">{{ ch.kids.length }} 点</span>
                    </button>
                    <label
                      class="full-toggle"
                      :class="{ on: chapterCurrent(s.key, t.id, idx) && getSel(s.key, t.id).fullCurrent }"
                    >
                      <input
                        type="checkbox"
                        :checked="chapterCurrent(s.key, t.id, idx) && getSel(s.key, t.id).fullCurrent"
                        @change="toggleFull(s.key, t.id, idx)"
                      />
                      <span>整章学完</span>
                    </label>
                  </div>

                  <!-- 当前章：展开知识点供逐个勾选 -->
                  <div v-if="chapterCurrent(s.key, t.id, idx) && ch.kids.length" class="kid-list">
                    <label v-for="k in ch.kids" :key="k.id" class="kid-row">
                      <input
                        type="checkbox"
                        :checked="sels[selKey(s.key, t.id)]?.checked.has(k.id)"
                        @change="toggleKid(s.key, t.id, k.id)"
                      />
                      <span class="kid-title">{{ k.title }}</span>
                    </label>
                  </div>
                </div>
              </div>
            </template>
          </div>
        </template>
      </div>
    </div>

    <template #footer>
      <span class="foot-note">总专业课进度表会自动随各教材更新</span>
      <Button variant="ghost" size="sm" :disabled="applying" @click="emitClose">取消</Button>
      <Button variant="primary" size="sm" :loading="applying" @click="apply">应用</Button>
    </template>
  </Modal>
</template>

<style scoped>
.batch-body {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  max-height: calc(100vh - 220px);
  overflow-y: auto;
  padding-right: 2px;
}
.round-bar {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-wrap: wrap;
}
.bar-label {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
}
.seg {
  display: inline-flex;
  gap: 2px;
  padding: 3px;
  background: var(--bg-tertiary);
  border-radius: var(--radius-full);
}
.seg-btn {
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  font-family: inherit;
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  padding: 4px 16px;
  border-radius: var(--radius-full);
  cursor: pointer;
  transition: all var(--transition-fast);
}
.seg-btn.active {
  background: var(--bg-elevated);
  color: var(--accent);
  box-shadow: var(--shadow-sm);
}
.bar-hint { font-size: var(--text-xs); color: var(--text-tertiary); }
.summary-line {
  margin: 0;
  font-size: var(--text-xs);
  color: var(--accent);
}
.error-banner {
  padding: 8px 12px;
  border-radius: 8px;
  background: var(--color-danger-subtle);
  border: 1px solid var(--color-danger);
  color: var(--color-danger);
  font-size: 12px;
}
.success-msg {
  margin: 0;
  font-size: var(--text-xs);
  color: var(--color-success, #16a34a);
}
.subj-block {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}
.subj-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding-top: var(--space-2);
  border-top: 1px solid var(--divider-color);
}
.subj-variant { font-size: var(--text-xs); color: var(--text-tertiary); }
.table-block {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  padding: var(--space-3);
  background: var(--bg-elevated);
}
.table-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-wrap: wrap;
}
.book-icon { color: var(--accent); flex-shrink: 0; }
.table-name {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.table-cov {
  margin-left: auto;
  font-size: var(--text-xs);
  color: var(--text-secondary);
  flex-shrink: 0;
}
.reset-btn {
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  font-family: inherit;
  font-size: var(--text-xs);
  cursor: pointer;
  padding: 2px 4px;
  border-radius: var(--radius-xs);
  flex-shrink: 0;
}
.reset-btn:hover { color: var(--color-danger); background: var(--bg-tertiary); }
.chapters {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 300px;
  overflow-y: auto;
}
.chapter-row {
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
  transition: border-color var(--transition-fast), background var(--transition-fast);
}
.chapter-row:hover { border-color: var(--border-color-strong); }
.chapter-row.current { border-color: var(--accent); }
.chapter-row.covered:not(.current) { opacity: 0.85; }
.chapter-line {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 6px 10px;
}
.chapter-main {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  cursor: pointer;
  padding: 0;
  font-family: inherit;
  text-align: left;
}
.ch-ic { flex-shrink: 0; }
.chapter-row.covered .ch-ic { color: var(--color-success, #16a34a); }
.chapter-row.current .ch-ic { color: var(--accent); }
.chapter-row:not(.current):not(.covered) .ch-ic { color: var(--text-quaternary); }
.ch-title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--text-sm);
  color: var(--text-primary);
}
.ch-count { font-size: var(--text-xs); color: var(--text-tertiary); flex-shrink: 0; }
.full-toggle {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  cursor: pointer;
  flex-shrink: 0;
  user-select: none;
}
.full-toggle:hover { color: var(--text-secondary); }
.full-toggle.on { color: var(--accent); }
.full-toggle input { accent-color: var(--accent); }
.kid-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 4px 10px 8px 30px;
  border-top: 1px dashed var(--divider-color);
  background: var(--bg-tertiary);
  border-radius: 0 0 var(--radius-sm) var(--radius-sm);
}
.kid-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 3px 4px;
  border-radius: var(--radius-xs);
  cursor: pointer;
}
.kid-row:hover { background: var(--bg-elevated); }
.kid-row input { accent-color: var(--accent); flex-shrink: 0; }
.kid-title {
  font-size: var(--text-xs);
  color: var(--text-secondary);
  min-width: 0;
}
.master-note {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-wrap: wrap;
  padding: var(--space-2) var(--space-3);
  background: var(--accent-subtle);
  border: 1px dashed var(--accent-soft, var(--border-color-strong));
  border-radius: var(--radius-md);
}
.master-name { font-size: var(--text-sm); font-weight: var(--font-medium); color: var(--text-primary); }
.master-tip { font-size: var(--text-xs); color: var(--text-secondary); }
.foot-note {
  margin-right: auto;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}
</style>