<script setup lang="ts">
/**
 * 「批量更改进度」弹窗
 *
 * 快速调整各科每本进度表的进度：选择「学到第几章」，之前的章节自动全部推进；
 * 未学完的章节点开章节细则逐个勾选已学知识点；顶部选择本轮状态（基础 / 强化）。
 * 专业课内置场景下，总专业课进度表由各教材覆盖度自动推导（本弹窗不列总表供手动编辑）。
 * 提交后走后端 batch_update_progress（只升不降），父组件收到 applied 事件后刷新。
 *
 * 交互细节：
 * - 章节可展开/收起（行首箭头，或点击当前章行头切换收起/展开）；
 * - 「整章学完」按章节持久记录，切换当前章后前章的勾选不会消失；
 * - 标记「整章学完」时该章知识点同步显示为已勾选（与保存结果一致）。
 */
import { computed, ref, reactive, watch } from "vue";
import * as api from "@/api";
import Button from "@/components/ui/Button.vue";
import Badge from "@/components/ui/Badge.vue";
import Modal from "@/components/ui/Modal.vue";
import Checkbox from "@/components/ui/Checkbox.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import { BookOpen, CheckCircle2, ChevronDown, ChevronRight, Circle, CircleDot } from "lucide-vue-next";
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

/** 本轮状态：学习中 / 基础 / 强化 */
const ROUNDS: { value: ProgressNodeStatus; label: string; hint: string }[] = [
  { value: "learning", label: "学习中", hint: "当前正在进行第一遍学习" },
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

/** 每张表的选中状态：key = `${subject}:${tableId}` */
interface TableSel {
  /** 学到第几章（chapters 下标）；null = 未选择 */
  reached: number | null;
  /** 显式标记「整章学完」的章节下标集合（跨章节持久，不因切换当前章而消失） */
  done: Set<number>;
  /** 当前章内已勾选的知识点 id（只对当前章生效） */
  checked: Set<string>;
  /** 已折叠的章节下标集合 */
  collapsed: Set<number>;
}
const sels = reactive<Record<string, TableSel>>({});

function selKey(subject: string, tableId: string): string {
  return `${subject}:${tableId}`;
}
function getSel(subject: string, tableId: string): TableSel {
  const k = selKey(subject, tableId);
  if (!sels[k]) sels[k] = { reached: null, done: new Set(), checked: new Set(), collapsed: new Set() };
  return sels[k];
}
function clearSel(subject: string, tableId: string) {
  const k = selKey(subject, tableId);
  if (sels[k]) delete sels[k];
}
function resetAll() {
  Object.keys(sels).forEach((k) => delete sels[k]);
  error.value = "";
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
function tableById(subject: string, tableId: string): ProgressTable | undefined {
  return tablesOf(subject).find((t) => t.id === tableId);
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
    if (sel.done.has(sel.reached)) n += cur.kids.length;
    else n += cur.kids.filter((k) => sel.checked.has(k.id)).length;
  }
  return n;
}

// ── 章节行状态 ──
function chapterCurrent(subject: string, tableId: string, idx: number): boolean {
  return getSel(subject, tableId).reached === idx;
}
function chapterCovered(subject: string, tableId: string, idx: number): boolean {
  const sel = getSel(subject, tableId);
  return (sel.reached != null && idx < sel.reached) || (sel.reached === idx && sel.done.has(idx));
}
function chapterIcon(subject: string, tableId: string, idx: number) {
  if (chapterCovered(subject, tableId, idx)) return CheckCircle2;
  if (chapterCurrent(subject, tableId, idx)) return CircleDot;
  return Circle;
}
function isChapterExpanded(subject: string, tableId: string, idx: number): boolean {
  return !getSel(subject, tableId).collapsed.has(idx);
}
function toggleCollapse(subject: string, tableId: string, idx: number) {
  const sel = getSel(subject, tableId);
  if (sel.collapsed.has(idx)) sel.collapsed.delete(idx);
  else sel.collapsed.add(idx);
}
/** 点击章节行：非当前章设为「学到本章」并展开；当前章再点一次则收起/展开 */
function pickChapter(subject: string, tableId: string, idx: number) {
  const sel = getSel(subject, tableId);
  if (sel.reached === idx) {
    toggleCollapse(subject, tableId, idx);
    return;
  }
  sel.reached = idx;
  sel.collapsed.delete(idx);
}
/** 「整章学完」：标记持久保存；标记时自动勾选该章全部知识点（与保存结果一致） */
function toggleDone(subject: string, tableId: string, idx: number) {
  const t = tableById(subject, tableId);
  if (!t) return;
  const sel = getSel(subject, tableId);
  sel.reached = idx;
  sel.collapsed.delete(idx);
  if (sel.done.has(idx)) {
    sel.done.delete(idx);
  } else {
    sel.done.add(idx);
    const ch = chaptersOf(t)[idx];
    if (ch) ch.kids.forEach((k) => sel.checked.add(k.id));
  }
}
function toggleKid(subject: string, tableId: string, kidId: string) {
  const sel = getSel(subject, tableId);
  if (sel.checked.has(kidId)) sel.checked.delete(kidId);
  else sel.checked.add(kidId);
}
/** 知识点勾选框是否可交互：仅当前章且未整章学完时可勾选 */
function kidDisabled(subject: string, tableId: string, idx: number): boolean {
  const sel = getSel(subject, tableId);
  return !(sel.reached === idx && !sel.done.has(idx));
}
/** 知识点勾选状态：当前章（未整章学完）→ 用户勾选；已覆盖/整章学完 → 显示为已勾选 */
function kidChecked(subject: string, tableId: string, idx: number, kidId: string): boolean {
  const sel = getSel(subject, tableId);
  if (sel.reached === idx && !sel.done.has(idx)) return sel.checked.has(kidId);
  return chapterCovered(subject, tableId, idx);
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
      const full = sel.done.has(sel.reached);
      list.push({
        table_id: t.id,
        reached_chapter: cur.id,
        current_full: full,
        current_points: full
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
  try {
    const res = await api.batchUpdateProgress(updates);
    emit("applied", res);
    emit("close"); // 应用成功后自动关闭弹窗
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    applying.value = false;
  }
}

function emitClose() {
  error.value = "";
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
        cov += sel.done.has(sel.reached)
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
                    <button
                      type="button"
                      class="collapse-btn"
                      :title="isChapterExpanded(s.key, t.id, idx) ? '收起' : '展开'"
                      @click="toggleCollapse(s.key, t.id, idx)"
                    >
                      <component
                        :is="isChapterExpanded(s.key, t.id, idx) ? ChevronDown : ChevronRight"
                        :size="14"
                      />
                    </button>
                    <button type="button" class="chapter-main" @click="pickChapter(s.key, t.id, idx)">
                      <component :is="chapterIcon(s.key, t.id, idx)" :size="14" class="ch-ic" />
                      <span class="ch-title" :title="ch.title">{{ ch.title }}</span>
                      <span class="ch-count">{{ ch.kids.length }} 点</span>
                    </button>
                    <label
                      v-if="chapterCurrent(s.key, t.id, idx)"
                      class="full-toggle"
                      :class="{ on: getSel(s.key, t.id).done.has(idx) }"
                    >
                      <Checkbox
                        :checked="getSel(s.key, t.id).done.has(idx)"
                        @change="toggleDone(s.key, t.id, idx)"
                      />
                      <span>整章学完</span>
                    </label>
                  </div>

                  <!-- 展开章节：显示其知识点（当前章可勾选；已覆盖/整章学完自动显示为已勾选） -->
                  <div v-if="isChapterExpanded(s.key, t.id, idx) && ch.kids.length" class="kid-list">
                    <label
                      v-for="k in ch.kids"
                      :key="k.id"
                      class="kid-row"
                      :class="{ readonly: kidDisabled(s.key, t.id, idx) }"
                    >
                      <Checkbox
                        :checked="kidChecked(s.key, t.id, idx, k.id)"
                        :disabled="kidDisabled(s.key, t.id, idx)"
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
  gap: var(--space-1);
  padding: 4px 8px;
}
.collapse-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border: none;
  background: transparent;
  color: var(--text-quaternary);
  border-radius: var(--radius-xs);
  cursor: pointer;
  flex-shrink: 0;
  padding: 0;
  transition: color var(--transition-fast), background var(--transition-fast);
}
.collapse-btn:hover { color: var(--text-primary); background: var(--bg-tertiary); }
.chapter-main {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  cursor: pointer;
  padding: 2px 2px;
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
  padding: 4px 10px 8px 34px;
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
.kid-row.readonly { cursor: default; opacity: 0.9; }
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