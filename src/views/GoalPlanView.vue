<script setup lang="ts">
/**
 * 「目标计划」页：用「截止日 + 目标章节」为各科安排每天的任务。
 *
 * 行为（由后端负责确定性倒排，前端仅做区间管理）：
 * - 某科存在生效区间时，其每日任务由后端按章节顺序表倒排生成；
 *   超过截止日 / 达标 / 已过期则自动回退到默认「按学习时长」安排。
 * - 复盘后按「实际进度 vs 目标差距」自动重排后续任务：完成多→减量、少→增量、
 *   提前达标→提前退出区间。
 *
 * 本页负责：查看/新建/编辑/删除各科的目标区间。
 */
import { ref, computed, watch, onMounted } from "vue";
import * as api from "@/api";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import Button from "@/components/ui/Button.vue";
import Badge from "@/components/ui/Badge.vue";
import Select from "@/components/ui/Select.vue";
import DatePicker from "@/components/ui/DatePicker.vue";
import { Plus, Trash2, Edit3, Target } from "lucide-vue-next";
import type { Goal, SubjectKey, ProgressIndex, ProgressTable } from "@/types";

const SUBJECTS: { key: SubjectKey; label: string }[] = [
  { key: "math", label: "数学" },
  { key: "english", label: "英语" },
  { key: "politics", label: "政治" },
  { key: "professional", label: "专业课" },
];

const subjectLabel = (k: SubjectKey) => SUBJECTS.find((s) => s.key === k)?.label ?? k;

const loading = ref(true);
const saving = ref(false);
const error = ref("");
const goals = ref<Goal[]>([]);

const editingId = ref<string | null>(null);
const formSubject = ref<SubjectKey>("math");
const formTitle = ref("");
const formDeadline = ref("");
const formTargetChapter = ref("");
const formStartChapter = ref("");

// ── 章节选项：取自该科目进度表（优先启用表，其次第一张表） ──
const progressIndex = ref<ProgressIndex | null>(null);

/** 当前科目用于章节选择的进度表 */
const activeProgressTable = computed<ProgressTable | null>(() => {
  const s = progressIndex.value?.subjects[formSubject.value];
  if (!s) return null;
  if (s.active_id) {
    const t = s.tables.find((x) => x.id === s.active_id);
    if (t) return t;
  }
  return s.tables[0] ?? null;
});

/** 章节选项（分组）：内置考纲的 chapter 节点是书本/板块级（如「高等数学」），具体章节/知识点
 * 是其下的 knowledge 子节点；后端按顺序表条目定位，书本名无法定位，故选项取「分组(书本/板块)
 * → 具体条目」两级。chapter 无子节点时（自定义表可能直接把章节建在 chapter 层）用自身兜底。
 * 旧表无 chapter 节点时退化为单组（phase 去重 / 全部节点标题）。 */
const chapterGroups = computed<{ group: string; items: string[] }[]>(() => {
  const t = activeProgressTable.value;
  if (!t) return [];
  const chapters = t.nodes.filter((n) => n.level === "chapter");
  if (chapters.length) {
    const groups: { group: string; items: string[] }[] = [];
    for (const ch of chapters) {
      const items = [
        ...new Set(
          t.nodes
            .filter((n) => n.level === "knowledge" && n.parent_id === ch.id)
            .map((n) => n.title)
            .filter((s) => s.trim() !== "")
        ),
      ];
      if (items.length) groups.push({ group: ch.title, items });
      else if (ch.title.trim()) groups.push({ group: "", items: [ch.title] });
    }
    return groups;
  }
  const phases = [...new Set(t.nodes.map((n) => n.phase).filter((p) => p.trim() !== ""))];
  if (phases.length) return [{ group: "", items: phases }];
  return [{ group: "", items: t.nodes.map((n) => n.title) }];
});

/** 是否有可选章节（无进度表时为空） */
const hasChapterOptions = computed(() => chapterGroups.value.some((g) => g.items.length > 0));

// 切换科目时清空已选的章节（选项来自别的进度表）
watch(formSubject, () => {
  formTargetChapter.value = "";
  formStartChapter.value = "";
});

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

async function reload() {
  loading.value = true;
  error.value = "";
  try {
    const [file, pidx] = await Promise.all([api.listGoals(), api.listProgressTables()]);
    goals.value = file?.data?.goals ?? [];
    progressIndex.value = pidx;
  } catch (e) {
    error.value = `加载目标区间失败：${errMsg(e)}`;
  } finally {
    loading.value = false;
  }
}

function statusMeta(status: string): { variant: "success" | "info" | "default"; label: string } {
  if (status === "completed") return { variant: "success", label: "已达标" };
  if (status === "expired") return { variant: "info", label: "已过期" };
  return { variant: "default", label: "进行中" };
}

function progressOf(g: Goal): number {
  const cur = g.current_position ?? 0;
  const tgt = g.target_position ?? 0;
  if (tgt <= 0) return 0;
  return Math.min(100, Math.round((cur / tgt) * 100));
}

function startEdit(g: Goal) {
  editingId.value = g.id;
  formSubject.value = g.subject;
  formTitle.value = g.title;
  formDeadline.value = g.deadline;
  formTargetChapter.value = g.target_chapter;
  formStartChapter.value = g.start_chapter ?? "";
}

function resetForm() {
  editingId.value = null;
  formSubject.value = "math";
  formTitle.value = "";
  formDeadline.value = "";
  formTargetChapter.value = "";
  formStartChapter.value = "";
}

async function save() {
  if (!formTitle.value.trim() || !formDeadline.value || !formTargetChapter.value.trim()) {
    error.value = "请完整填写目标描述、截止日期与目标章节。";
    return;
  }
  saving.value = true;
  error.value = "";
  try {
    if (editingId.value) {
      const existing = goals.value.find((g) => g.id === editingId.value);
      if (existing) {
        await api.updateGoal({
          ...existing,
          subject: formSubject.value,
          title: formTitle.value.trim(),
          deadline: formDeadline.value,
          target_chapter: formTargetChapter.value.trim(),
          start_chapter: formStartChapter.value.trim() || undefined,
        });
      }
    } else {
      await api.createGoal(
        formSubject.value,
        formTitle.value.trim(),
        formDeadline.value,
        formTargetChapter.value.trim(),
        formStartChapter.value.trim() || undefined,
      );
    }
    resetForm();
    await reload();
  } catch (e) {
    error.value = `保存失败：${errMsg(e)}`;
  } finally {
    saving.value = false;
  }
}

async function remove(g: Goal) {
  if (!window.confirm(`确定删除「${g.title}」？该科目将从截止日模式回退到按学习时长安排。`)) return;
  try {
    await api.deleteGoal(g.id);
    if (editingId.value === g.id) resetForm();
    await reload();
  } catch (e) {
    error.value = `删除失败：${errMsg(e)}`;
  }
}

const sortedGoals = computed(() => {
  return [...goals.value].sort((a, b) => {
    if (a.status !== b.status) return a.status === "active" ? -1 : 1;
    return a.deadline.localeCompare(b.deadline);
  });
});

onMounted(reload);
</script>

<template>
  <div class="goal-page">
    <header class="page-head">
      <h1 class="page-title">目标计划</h1>
      <p class="page-sub">
        为某科设置「截止日 + 目标章节」后，区间内每日任务由目标倒排生成；到达截止日、
        达标或已过期即回退到按学习时长安排。复盘会按实际进度自动增删后续任务。
      </p>
      <div v-if="error" class="error-banner" role="alert">{{ error }}</div>
    </header>

    <LoadingSpinner v-if="loading" :size="30" label="加载目标区间..." class="page-loading" />

    <div v-else class="content">
      <!-- 新建 / 编辑表单 -->
      <section class="form-card">
        <h2 class="form-title">
          <Target :size="16" />
          {{ editingId ? "编辑目标区间" : "新建目标区间" }}
        </h2>
        <div class="form-grid">
          <div class="field">
            <label class="field-label">科目</label>
            <Select v-model="formSubject">
              <option v-for="s in SUBJECTS" :key="s.key" :value="s.key">{{ s.label }}</option>
            </Select>
          </div>
          <div class="field">
            <label class="field-label">截止日期</label>
            <DatePicker v-model="formDeadline" placeholder="选择截止日期" />
          </div>
          <div class="field field-wide">
            <label class="field-label">目标描述（如 "9/20 前完成线性方程组"）</label>
            <input v-model="formTitle" type="text" class="text-input" placeholder="例：9/20 前完成线性方程组" />
          </div>
          <div class="field">
            <label class="field-label">目标章节（从进度表选择具体章节）</label>
            <Select v-model="formTargetChapter" :placeholder="hasChapterOptions ? '选择目标章节' : '暂无进度表'">
              <template v-if="hasChapterOptions">
                <optgroup v-for="(g, gi) in chapterGroups" :key="g.group || `g${gi}`" :label="g.group">
                  <option v-for="c in g.items" :key="c" :value="c">{{ c }}</option>
                </optgroup>
              </template>
              <option v-else value="" disabled>暂无进度表，请先到「进度」页创建</option>
            </Select>
            <p v-if="activeProgressTable" class="field-hint">选自「{{ activeProgressTable.name }}」</p>
          </div>
          <div class="field">
            <label class="field-label">起始章节（可选，默认从0起点）</label>
            <Select v-model="formStartChapter" placeholder="从当前起点开始">
              <option value="">从当前起点开始</option>
              <optgroup v-for="(g, gi) in chapterGroups" :key="g.group || `g${gi}`" :label="g.group">
                <option v-for="c in g.items" :key="c" :value="c">{{ c }}</option>
              </optgroup>
            </Select>
            <p v-if="activeProgressTable" class="field-hint">选自「{{ activeProgressTable.name }}」</p>
          </div>
        </div>
        <div class="form-actions">
          <Button v-if="editingId" variant="ghost" size="sm" @click="resetForm">取消</Button>
          <Button :variant="editingId ? 'secondary' : 'primary'" size="sm" :loading="saving" @click="save">
            <Plus :size="14" />
            {{ editingId ? "保存修改" : "添加区间" }}
          </Button>
        </div>
      </section>

      <!-- 区间列表 -->
      <section class="list">
        <EmptyState
          v-if="sortedGoals.length === 0"
          title="还没有目标区间"
          description="点上方「新建目标区间」，为某科设置截止日与目标章节，开始按目标安排每天任务。"
        />
        <div v-else class="goal-cards">
          <div v-for="g in sortedGoals" :key="g.id" class="goal-card" :class="{ inactive: g.status !== 'active' }">
            <div class="goal-row">
              <Badge :variant="g.subject" size="md">{{ subjectLabel(g.subject) }}</Badge>
              <Badge :variant="statusMeta(g.status).variant">{{ statusMeta(g.status).label }}</Badge>
              <span class="goal-title">{{ g.title }}</span>
              <div class="goal-actions">
                <button type="button" class="icon-btn" title="编辑" @click="startEdit(g)">
                  <Edit3 :size="15" />
                </button>
                <button type="button" class="icon-btn danger" title="删除" @click="remove(g)">
                  <Trash2 :size="15" />
                </button>
              </div>
            </div>
            <div class="goal-meta">
              <span class="meta-chip">截止 {{ g.deadline }}</span>
              <span class="meta-chip" v-if="g.start_chapter">起点：{{ g.start_chapter }}</span>
              <span class="meta-chip">目标：{{ g.target_chapter }}</span>
            </div>
            <div class="progress-track">
              <div class="progress-fill" :style="{ width: progressOf(g) + '%' }" />
            </div>
            <div class="progress-label">进度 {{ progressOf(g) }}%</div>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.goal-page {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  padding: var(--space-5) var(--space-6);
  gap: var(--space-5);
  overflow-y: auto;
}
.page-head {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  flex-shrink: 0;
}
.page-title {
  margin: 0;
  font-size: var(--text-2xl);
  font-weight: var(--font-bold);
  color: var(--text-primary);
  letter-spacing: -0.02em;
}
.page-sub {
  margin: 0;
  font-size: var(--text-sm);
  color: var(--text-tertiary);
  max-width: 720px;
  line-height: 1.5;
}
.error-banner {
  margin-top: var(--space-2);
  padding: 8px 12px;
  border-radius: 8px;
  background: var(--color-danger-subtle);
  border: 1px solid var(--color-danger);
  color: var(--color-danger);
  font-size: 12px;
}
.page-loading {
  margin: auto;
}

.form-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-4);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-lg);
  background: var(--bg-elevated);
  flex-shrink: 0;
}
.form-title {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin: 0;
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}
.form-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: var(--space-3);
}
.field {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}
.field-wide {
  grid-column: 1 / -1;
}
.field-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}
.field-hint {
  margin: 0;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.text-input {
  min-height: 36px;
  padding: var(--space-2) var(--space-3);
  font-family: inherit;
  font-size: var(--text-sm);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-full);
  background: var(--bg-elevated);
  color: var(--text-primary);
  outline: none;
  transition: border-color var(--transition-fast);
}
.text-input:focus {
  border-color: var(--accent);
}
.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
}

.list {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}
.goal-cards {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}
.goal-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-4);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-lg);
  background: var(--bg-elevated);
}
.goal-card.inactive {
  opacity: 0.65;
}
.goal-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-wrap: wrap;
}
.goal-title {
  flex: 1;
  min-width: 160px;
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}
.goal-actions {
  display: flex;
  gap: var(--space-1);
}
.icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border: none;
  border-radius: var(--radius-full);
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  transition: background var(--transition-fast), color var(--transition-fast);
}
.icon-btn:hover {
  background: var(--bg-overlay);
  color: var(--text-primary);
}
.icon-btn.danger:hover {
  background: var(--color-danger-subtle);
  color: var(--color-danger);
}
.goal-meta {
  display: flex;
  gap: var(--space-2);
  flex-wrap: wrap;
}
.meta-chip {
  font-size: var(--text-xs);
  color: var(--text-secondary);
  padding: 2px 10px;
  border-radius: var(--radius-full);
  background: var(--bg-tertiary);
}
.progress-track {
  height: 6px;
  border-radius: var(--radius-full);
  background: var(--bg-tertiary);
  overflow: hidden;
}
.progress-fill {
  height: 100%;
  border-radius: var(--radius-full);
  background: var(--accent);
  transition: width var(--transition-normal);
}
.progress-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}
</style>