<script setup lang="ts">
import { ref, computed, onMounted, watch, nextTick } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useTodayStore } from "@/stores/today";
import { useSettingsStore } from "@/stores/settings";
import { todayString, yesterdayString, prevDateString, weekdayName, getWeekStart } from "@/utils/date";
import * as api from "@/api";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import {
  CheckCircle2,
  Circle,
  AlertTriangle,
  ArrowRight,
  ArrowLeft,
  Check,
  Sparkles,
  Clock,
  Calendar,
  ChevronLeft,
  ChevronRight,
  History,
  Coffee,
  Ban,
  Smartphone,
  Info,
  Plus,
  FolderOpen,
  CircleDot,
} from "lucide-vue-next";
import type {
  TaskReviewEntry,
  DailyReviewInput,
  ReviewRecord,
  OvercompletionEntry,
  RegenDayChange,
  ProgressIndex,
  ProgressTable,
  ProgressNode,
  ProgressNodeStatus,
} from "@/types";

const todayStore = useTodayStore();
const settingsStore = useSettingsStore();
const route = useRoute();
const router = useRouter();

// ── Date navigation ──
const selectedDate = ref<string>(todayString());
const todayDate = todayString();
const yesterdayDate = computed(() => yesterdayString());

// 已有复盘的日期列表（用于跳转）
const reviewDates = ref<string[]>([]);

// 日期切换方向：用于过渡动画（prev=向右滑入，next=向左滑入）
const dateTransitionName = ref("date-slide-left");

function setDate(date: string, direction: "prev" | "next" | "jump" = "jump") {
  if (date === selectedDate.value) return;
  dateTransitionName.value = direction === "prev" ? "date-slide-right" : "date-slide-left";
  selectedDate.value = date;
  // 同步 URL query
  router.replace({ name: "review", query: { date } });
}

function goPrevDay() {
  setDate(prevDateString(selectedDate.value), "prev");
}

function goNextDay() {
  const [y, m, d] = selectedDate.value.split("-").map(Number);
  const dt = new Date(y, m - 1, d, 12, 0, 0);
  dt.setDate(dt.getDate() + 1);
  const next = `${dt.getFullYear()}-${String(dt.getMonth() + 1).padStart(2, "0")}-${String(dt.getDate()).padStart(2, "0")}`;
  // 不允许超过今天
  if (next <= todayDate) {
    setDate(next, "next");
  }
}

function goToday() {
  setDate(todayDate);
}

const isToday = computed(() => selectedDate.value === todayDate);
const isYesterday = computed(() => selectedDate.value === yesterdayDate.value);
const isFuture = computed(() => selectedDate.value > todayDate);
const isOlderThanYesterday = computed(() =>
  selectedDate.value < yesterdayDate.value && selectedDate.value < todayDate
);

// ── 休息日 / 排除日判断 ──
const isCurrentDateRestDay = computed(() => {
  const restDays = settingsStore.settings?.study_schedule?.rest_days ?? ["周日"];
  return restDays.includes(weekdayName(selectedDate.value));
});
const selectedDateExcluded = ref(false);
const selectedDateExcludedReason = ref<string | null>(null);

async function checkSelectedDateExcluded() {
  selectedDateExcluded.value = false;
  selectedDateExcludedReason.value = null;
  try {
    const wp = await api.getWeekPlan(getWeekStart(selectedDate.value));
    const ex = wp.data?.excluded_days?.find((d) => d.date === selectedDate.value);
    if (ex) {
      selectedDateExcluded.value = true;
      const label = { travel: "外出旅行", sick: "生病", exam: "考试", other: "其他" }[ex.reason_type] ?? "特殊情况";
      selectedDateExcludedReason.value = ex.note ? `${label}（${ex.note}）` : label;
    }
  } catch {
    // 无周计划，忽略
  }
}

// ── State ──
const step = ref(0);
const totalSteps = 5;
const quickMode = ref(false);
const submitting = ref(false);
const loading = ref(true);

// 提交复盘的加载进度：展示给用户当前执行到哪个阶段
// 数组顺序即展示顺序。每条步骤都必须对应后端的真实工作：
// 旧版「正在应用修改」在 submit_review 返回后由前端瞬时切步（耗时恒为 0ms），
// 用户看不到，改由「正在保存复盘」一并覆盖（保存复盘文件 + 更新 State 状态）。
interface SubmitStep { key: string; label: string; }
const SUBMIT_STEPS: SubmitStep[] = [
  { key: "analyzing", label: "正在保存复盘" },
  { key: "adjusting", label: "正在调整" },
  { key: "syncing", label: "正在同步滴答" },
];
/** 无需 AI 调整后续计划时的默认步骤（「正在调整」按需插入） */
const DEFAULT_SUBMIT_STEP_KEYS = ["analyzing", "syncing"];
/** 按 key 构造步骤列表：避免用数组下标硬编码步骤（下标会随步骤增删而失效） */
function buildSubmitSteps(keys: string[]): SubmitStep[] {
  return keys
    .map(key => SUBMIT_STEPS.find(s => s.key === key))
    .filter((s): s is SubmitStep => !!s);
}
/** 当前展示的步骤列表（无需 AI 调整时动态省略「正在调整」步骤） */
const submitSteps = ref<SubmitStep[]>(buildSubmitSteps(DEFAULT_SUBMIT_STEP_KEYS));
/** 当前进度步骤索引：-1 表示不在提交/调整流程中（不显示进度卡片） */
const submitStepIndex = ref(-1);
/** 单步最短展示时长（ms）：步骤对应的工作过快时补足，避免进度条一闪而过 */
const SUBMIT_STEP_MIN_MS = 450;
let submitStepShownAt = 0;
function setSubmitStep(i: number) {
  submitStepIndex.value = i;
  submitStepShownAt = Date.now();
}
/** 推进到下一步：若当前步骤展示时间不足最短时长，先补足再切换 */
async function advanceSubmitStep(i: number) {
  const shownFor = Date.now() - submitStepShownAt;
  if (submitStepIndex.value >= 0 && i !== submitStepIndex.value && shownFor < SUBMIT_STEP_MIN_MS) {
    await new Promise(resolve => setTimeout(resolve, SUBMIT_STEP_MIN_MS - shownFor));
  }
  setSubmitStep(i);
}
/** 确保某步骤存在于当前列表（重试/恢复时补充），返回其在列表中的索引 */
function ensureSubmitStep(key: string): number {
  let idx = submitSteps.value.findIndex(s => s.key === key);
  if (idx === -1) {
    const step = SUBMIT_STEPS.find(s => s.key === key);
    if (step) {
      submitSteps.value.push(step);
      // 按 SUBMIT_STEPS 的规范顺序重排：动态插入的步骤不能排到「同步滴答」之后
      const order = new Map(SUBMIT_STEPS.map((s, i) => [s.key, i]));
      submitSteps.value.sort((a, b) => (order.get(a.key) ?? 0) - (order.get(b.key) ?? 0));
    }
    idx = submitSteps.value.findIndex(s => s.key === key);
  }
  return idx;
}
const existingReview = ref<ReviewRecord | null>(null);
const submitted = ref(false);

// 重排状态：复盘提交后若需要 AI 重新生成剩余天数计划
// 使用 sessionStorage 持久化，避免切页再回来时丢失提示
const REGEN_SESSION_KEY = "studyagent.regen_status";
interface RegenStatus {
  date: string;
  regenerating: boolean;
  message: string;
  failed: boolean;
  timestamp: number;
  /** 各受影响日期的任务变动明细（悬停展示；持久化便于切页/切日期后仍常驻） */
  changes?: RegenDayChange[];
}
function loadRegenStatus(): RegenStatus | null {
  try {
    const raw = sessionStorage.getItem(REGEN_SESSION_KEY);
    if (!raw) return null;
    const status = JSON.parse(raw) as RegenStatus;
    // 仅保留 24 小时内的状态：重排提示在当天应常态化可见，过期视为陈旧
    if (Date.now() - status.timestamp > 24 * 60 * 60 * 1000) {
      sessionStorage.removeItem(REGEN_SESSION_KEY);
      return null;
    }
    return status;
  } catch {
    return null;
  }
}
function saveRegenStatus(status: RegenStatus) {
  sessionStorage.setItem(REGEN_SESSION_KEY, JSON.stringify(status));
}
function clearRegenStatus() {
  sessionStorage.removeItem(REGEN_SESSION_KEY);
}

const initialRegen = loadRegenStatus();
// 仅当日期匹配且仍在 regenerating 时恢复，避免错误显示历史状态
const regenerating = ref(initialRegen?.regenerating ?? false);
const regenMessage = ref(initialRegen?.message ?? "");
// 需求：失败标记同样持久化，切页再回来仍保留重试按钮
const regenFailed = ref(initialRegen?.failed ?? false);
// 重排变更明细（悬停展示）：持久化，切页 / 切日期后再回来仍常驻展示
const regenChanges = ref<RegenDayChange[]>(initialRegen?.changes ?? []);

// 计划外学习：用户实际进度领先计划或做了计划外学习时填写
const hasOvercompletion = ref(false);
const overcompletions = ref<OvercompletionEntry[]>([]);

// ── Time gate ──
const endTime = computed(() => settingsStore.settings?.study_schedule?.end_time ?? "22:00");
const startTime = computed(() => settingsStore.settings?.study_schedule?.start_time ?? "09:00");
const beforeEndTime = computed(() => {
  if (!isToday.value) return false;
  const now = new Date();
  const [h, m] = endTime.value.split(":").map(Number);
  const end = new Date(now);
  end.setHours(h, m, 0, 0);
  return now < end;
});
// 当前时间是否在今天的学习开始时间之前（用于补复盘时间窗）
const beforeStartTimeToday = computed(() => {
  const now = new Date();
  const [h, m] = startTime.value.split(":").map(Number);
  const start = new Date(now);
  start.setHours(h, m, 0, 0);
  return now < start;
});

// ── 补复盘条件 ──
// 仅允许对「昨天」补复盘，且昨天没有复盘 + 当前时间在今天的学习开始时间之前
const canBackfill = computed(() =>
  isYesterday.value &&
  !existingReview.value &&
  beforeStartTimeToday.value
);

// 是否允许填写复盘表单
const canFillReview = computed(() => {
  if (isToday.value) {
    // 今天：需要在结束时间之后且无已有复盘
    return !beforeEndTime.value && !submitted.value;
  }
  if (canBackfill.value) {
    return true;
  }
  return false;
});

// 是否只读（查看历史复盘）
// isReadOnly 计算已移除（无引用）

// ── Tasks (from state, for the selected date) ──
const plan = computed(() => todayStore.plan);
const allTasks = computed(() => todayStore.allTasks);

// Step 1: task completion (read from state, initialized from task.status)
const taskCompleted = ref<Record<string, boolean>>({});
// 滴答清单回读命中数：手机端已勾选完成、在复盘页自动标记的任务数量
const didaMatchedCount = ref(0);

// Step 2: blockers (per incomplete Priority A task)
const taskBlockers = ref<Record<string, string[]>>({});
const blockerNotes = ref<Record<string, string>>({});

// Step 2: overall feeling
const overallFeeling = ref("normal");

// Step 5: main difficulty
const mainDifficulty = ref("");

// Step 4: 任务量合理性与临时外部异常
const workloadFeedback = ref("reasonable");
const externalInterference = ref("none");

// ── 任务计时（仅当设置启用 enable_time_tracking 时启用）──
const timeTrackingEnabled = computed(
  () => !!settingsStore.settings?.study_schedule?.enable_time_tracking
);

/** 每个任务的实际累计分钟数（从 State 读取，仅当前日匹配时有效） */
const taskActualMinutes = ref<Record<string, number>>({});

/** 格式化分钟为 "Xh Ym" 或 "Ym" */
function formatMinutes(min: number): string {
  if (min <= 0) return "0m";
  if (min < 60) return `${min}m`;
  const h = Math.floor(min / 60);
  const m = min % 60;
  return m > 0 ? `${h}h ${m}m` : `${h}h`;
}

/** 格式化小时为 "Xh" 或 "Xh Ym" */
function formatHours(hours: number): string {
  if (hours <= 0) return "0h";
  const totalMin = Math.round(hours * 60);
  return formatMinutes(totalMin);
}

/** 读取某任务在复盘记录中的估时（优先 task_reviews，回退当日计划） */
function taskEstimatedHours(taskId: string, tr?: TaskReviewEntry): number {
  if (tr?.estimated_hours != null && tr.estimated_hours > 0) return tr.estimated_hours;
  return allTasks.value.find(t => t.id === taskId)?.estimated_hours ?? 0;
}

/** 读取某任务在复盘记录中的实际用时（分钟） */
function taskActualFromReview(_taskId: string, tr?: TaskReviewEntry): number {
  return tr?.actual_minutes ?? 0;
}

// ── Computed ──
const incompleteTasks = computed(() => {
  return allTasks.value.filter(t => !taskCompleted.value[t.id]);
});

const doneTasks = computed(() => {
  return allTasks.value.filter(t => taskCompleted.value[t.id]);
});

// ── Labels ──
const blockerOptions = [
  { value: "time", label: "时间不足" },
  { value: "understanding", label: "理解困难" },
  { value: "practice", label: "练习不足" },
  { value: "memorization", label: "遗忘较多" },
  { value: "overload", label: "工作量安排过多" },
  { value: "interruption", label: "临时事务" },
  { value: "energy", label: "今天状态不好" },
  { value: "resource", label: "资源不足" },
  { value: "other", label: "其它" },
];

const feelingOptions = [
  { value: "smooth", label: "很顺利", icon: "😊" },
  { value: "normal", label: "一般", icon: "😐" },
  { value: "hard", label: "比较困难", icon: "😣" },
];

const workloadOptions = [
  { value: "too_little", label: "偏少" },
  { value: "reasonable", label: "合理" },
  { value: "too_much", label: "偏多" },
];

const externalOptions = [
  { value: "none", label: "无" },
  { value: "sick", label: "生病" },
  { value: "travel", label: "外出" },
  { value: "exam", label: "临时考试" },
  { value: "family", label: "家庭事务" },
  { value: "environment", label: "环境中断" },
  { value: "other", label: "其他" },
];

const difficultyOptions = [
  { value: "understanding", label: "理解概念" },
  { value: "problems", label: "做题" },
  { value: "memorization", label: "记忆" },
  { value: "attention", label: "注意力" },
  { value: "time_management", label: "时间安排" },
  { value: "environment", label: "学习环境" },
  { value: "other", label: "其它" },
];

function subjectLabel(s: string): string {
  const m: Record<string, string> = { math: "数学", english: "英语", politics: "政治", professional: "专业课" };
  return m[s] ?? s;
}

// 反查任务标题（用于复盘记录展示，优先用 task_reviews 自带的 title，再回退到当日计划）
function findTaskTitle(taskId: string, tr?: TaskReviewEntry): string {
  if (tr?.title) return tr.title;
  return allTasks.value.find(t => t.id === taskId)?.title ?? "(任务已删除)";
}

function findTaskSubject(taskId: string, tr?: TaskReviewEntry): string {
  if (tr?.subject) return tr.subject;
  return allTasks.value.find(t => t.id === taskId)?.subject ?? "";
}

// ── 计划外学习：基于进度表章节的选择器 ──
const OC_SUBJECTS = ["math", "english", "politics", "professional"] as const;

const progressIndex = ref<ProgressIndex | null>(null);
const progressLoading = ref(false);
const activeSubject = ref<string>("math");
const savingProgress = ref(false);
const newChapterTitle = ref("");
const newChapterError = ref("");
// 计划外学习 v2「进度指针」会话态：
// 以「完成区尾」（状态 ≥ 基础 的最后一个章节）为当前记录进度，
// 点选更靠后的章节把区间整段补记为本次计划外；点选完成区内章节则视为记录超前，确认后回退。
const correctionTarget = ref<ProgressNode | null>(null);
const ocHint = ref("");
/** 会话开始时各节点状态快照：`${subject}|${tableId}|${nodeId}` → status（撤销推进/回退时还原） */
const ocOrig = ref<Record<string, string>>({});
/** 各表进入会话时的完成区尾下标：`${subject}|${tableId}` → index */
const ocTail = ref<Record<string, number>>({});
const ocListRef = ref<HTMLElement | null>(null);

async function loadProgressIndex() {
  progressLoading.value = true;
  try {
    progressIndex.value = await api.listProgressTables();
  } catch {
    progressIndex.value = null;
  } finally {
    progressLoading.value = false;
  }
}

function subjectSet(subject: string) {
  return progressIndex.value?.subjects[subject];
}

/** 解析某科目当前生效的进度表（优先 active_id，其次 active_variant 下首张表，最后首张表） */
function resolveActiveTable(subject: string): ProgressTable | null {
  const set = subjectSet(subject);
  if (!set || !set.tables.length) return null;
  const byActive = set.tables.find((t) => t.id === set.active_id);
  if (byActive) return byActive;
  if (set.active_variant) {
    const vt = set.tables.find((t) => t.variant === set.active_variant);
    if (vt) return vt;
  }
  return set.tables[0] ?? null;
}

function hasTable(subject: string): boolean {
  return resolveActiveTable(subject) !== null;
}

/**
 * 当前查看的进度表（计划外学习选择器）：科目内多表时可自由切换查看/勾选
 * （专业课通常是「总进度表 + 各指定教材表」，其它科目也可能有多份考纲方案并存）。
 * 默认跟随该科目启用的表；用户点选具体表后记住选择。
 */
const viewTableIds = ref<Record<string, string>>({});

function tablesOfSubject(subject: string): ProgressTable[] {
  return subjectSet(subject)?.tables ?? [];
}

function viewTableOf(subject: string): ProgressTable | null {
  const set = subjectSet(subject);
  if (!set || !set.tables.length) return null;
  const chosenId = viewTableIds.value[subject];
  if (chosenId) {
    const t = set.tables.find((x) => x.id === chosenId);
    if (t) return t;
  }
  return resolveActiveTable(subject);
}

function setViewTable(subject: string, tableId: string) {
  const next = { ...viewTableIds.value };
  if (tableId) next[subject] = tableId;
  else delete next[subject];
  viewTableIds.value = next;
  newChapterTitle.value = "";
  newChapterError.value = "";
}

/** 某表内已被勾选（记录到 overcompletions）的条目数，用于表切换时提示 */
function ocCountOfTable(subject: string, table: ProgressTable): number {
  return overcompletions.value.filter(
    (oc) =>
      oc.subject === subject &&
      table.nodes.some(
        (n) => n.id === oc.node_id || (!oc.node_id && n.title === oc.chapter_reached)
      )
  ).length;
}

const activeTable = computed(() => viewTableOf(activeSubject.value));

/** 章节级节点（有章节时以章节为选择单位） */
const chapterNodes = computed<ProgressNode[]>(() =>
  (activeTable.value?.nodes ?? []).filter((n) => n.level === "chapter")
);
/** 知识点级节点（内置表无章节时回退为选择单位） */
const knowledgeNodes = computed<ProgressNode[]>(() =>
  (activeTable.value?.nodes ?? []).filter((n) => n.level === "knowledge")
);
const hasChapters = computed(() => chapterNodes.value.length > 0);

/** 章节 id → 其直属知识点 */
const knowledgeByChapter = computed(() => {
  const m = new Map<string, ProgressNode[]>();
  for (const k of knowledgeNodes.value) {
    if (!k.parent_id) continue;
    const arr = m.get(k.parent_id);
    if (arr) arr.push(k);
    else m.set(k.parent_id, [k]);
  }
  return m;
});
/** 未挂到任何章节下的知识点（孤儿节点，单独区段展示） */
const orphanKnowledge = computed(() =>
  knowledgeNodes.value.filter((k) => !k.parent_id)
);
function childrenOfChapter(chapterId: string): ProgressNode[] {
  return knowledgeByChapter.value.get(chapterId) ?? [];
}

function entryFor(subject: string, node: ProgressNode): OvercompletionEntry | undefined {
  return overcompletions.value.find(
    (oc) =>
      oc.subject === subject &&
      (oc.node_id === node.id || (!oc.node_id && oc.chapter_reached === node.title))
  );
}

function isChecked(subject: string, node: ProgressNode): boolean {
  return !!entryFor(subject, node);
}

/** 无法匹配任何进度表节点的历史条目（该科无表 / 节点已删除），保留为可编辑兜底行 */
const unmatchedEntries = computed(() =>
  overcompletions.value.filter((oc) => {
    const tables = tablesOfSubject(oc.subject);
    if (!tables.length) return true;
    // 只要能在该科任意一张表里匹配到就不算“未匹配”，避免切换查看表时历史记录被误判
    return !tables.some((t) =>
      t.nodes.some(
        (n) => n.id === oc.node_id || (!oc.node_id && n.title === oc.chapter_reached)
      )
    );
  })
);

/** 用返回值替换本地 index 中的表，避免整页重拉闪烁 */
function updateTableInIndex(subject: string, table: ProgressTable) {
  const set = progressIndex.value?.subjects[subject];
  if (!set) return;
  const i = set.tables.findIndex((t) => t.id === table.id);
  if (i >= 0) set.tables[i] = table;
}

/** 写回当前查看的进度表（makeActive=false；savingProgress 防重入避免整表覆盖丢失更新） */
async function persistActiveTable(subject: string, mutate: (t: ProgressTable) => void) {
  if (savingProgress.value) return;
  const t = viewTableOf(subject);
  if (!t) return;
  savingProgress.value = true;
  try {
    const copy: ProgressTable = { ...t, nodes: [...t.nodes] };
    mutate(copy);
    const saved = await api.saveProgressTable(subject, copy.variant, copy, false);
    updateTableInIndex(subject, saved);
  } catch (e) {
    console.error("保存进度表失败:", e);
  } finally {
    savingProgress.value = false;
  }
}

/**
 * 计划外学习只把节点状态推进到「基础」——超前/额外学了一遍并不等于真正掌握，
 * 之后的巩固与复盘再决定是否升到强化/掌握。已高于基础的节点不回退。
 */
const STATUS_RANK: Record<string, number> = {
  pending: 0,
  learning: 1,
  basic: 2,
  reinforcing: 3,
  mastered: 4,
};

function ocRankOf(n: ProgressNode): number {
  return STATUS_RANK[n.status] ?? 0;
}
/** 该节点在系统记录中是否已学过（≥ 基础） */
function ocIsLearned(n: ProgressNode): boolean {
  return ocRankOf(n) >= STATUS_RANK.basic;
}

/** 当前查看表内按顺序的可声明单位：有章节取章节，否则平铺知识点 */
const ocUnits = computed<ProgressNode[]>(() =>
  hasChapters.value ? chapterNodes.value : knowledgeNodes.value
);

function ocIndexOf(node: ProgressNode): number {
  return ocUnits.value.findIndex((u) => u.id === node.id);
}

/** 进入会话时为当前查看表锁定快照：完成区尾下标 + 各节点原始状态（幂等） */
function ensureOcSnapshot(subject: string) {
  const table = viewTableOf(subject);
  if (!table) return;
  const key = `${subject}|${table.id}`;
  if (key in ocTail.value) return;
  const units = ocUnits.value;
  let tail = -1;
  for (let i = 0; i < units.length; i++) {
    if (ocIsLearned(units[i])) tail = i;
  }
  ocTail.value[key] = tail;
  for (const n of table.nodes) {
    const k = `${subject}|${table.id}|${n.id}`;
    if (!(k in ocOrig.value)) ocOrig.value[k] = n.status;
  }
}

/** 当前查看表的完成区尾下标（进入会话时锁定；回退确认后更新） */
const ocTailIdx = computed<number>(() => {
  const subject = activeSubject.value;
  const table = activeTable.value;
  if (!table) return -1;
  ensureOcSnapshot(subject);
  return ocTail.value[`${subject}|${table.id}`] ?? -1;
});
/** 完成区最后一个已学章节（目前进度锚点） */
const ocTailUnit = computed<ProgressNode | null>(() => {
  const i = ocTailIdx.value;
  return i >= 0 ? ocUnits.value[i] ?? null : null;
});
/** 首个待学章节（自动定位目标） */
const ocNextUnit = computed<ProgressNode | null>(() => {
  const units = ocUnits.value;
  const i = ocTailIdx.value + 1;
  return units.length > 0 && i >= 0 && i < units.length ? units[i] : null;
});

function ocSnapshotStatus(subject: string, tableId: string, nodeId: string): ProgressNodeStatus {
  const s = ocOrig.value[`${subject}|${tableId}|${nodeId}`];
  return s === "pending" || s === "learning" || s === "basic" || s === "reinforcing" || s === "mastered"
    ? (s as ProgressNodeStatus)
    : "pending";
}

/** 撤销某单位在本轮的记录（按节点 id 或旧版标题匹配） */
function removeOcOf(subject: string, node: ProgressNode) {
  overcompletions.value = overcompletions.value.filter(
    (oc) =>
      oc.subject !== subject ||
      !(oc.node_id === node.id || (!oc.node_id && oc.chapter_reached === node.title))
  );
}

/** 本地副本上把单位(及其子知识点)推进到基础 / 还原到会话初始状态 */
function applyNodeState(
  subject: string,
  tableId: string,
  t: ProgressTable,
  nodeId: string,
  advance: boolean
) {
  const n = t.nodes.find((x) => x.id === nodeId);
  if (!n) return;
  const apply = (target: ProgressNode) => {
    if (advance) {
      if (ocRankOf(target) < STATUS_RANK.basic) target.status = "basic";
    } else {
      target.status = ocSnapshotStatus(subject, tableId, target.id);
    }
  };
  apply(n);
  if (n.level === "chapter") {
    for (const k of t.nodes) {
      if (k.parent_id === n.id) apply(k);
    }
  }
}

/** 前向推进：把 (完成区尾, 所选章] 整段作为本轮计划外声明（点选更早的本轮章节 = 回缩） */
async function applyForwardPointer(idx: number) {
  const subject = activeSubject.value;
  const table = activeTable.value;
  if (!table) return;
  ensureOcSnapshot(subject);
  const tail = ocTail.value[`${subject}|${table.id}`] ?? -1;
  if (idx <= tail) return;
  const units = ocUnits.value;
  const target = units[idx];
  if (!target) return;
  const desired = new Set(units.slice(tail + 1, idx + 1).map((u) => u.id));

  const toAdd = units.slice(tail + 1, idx + 1).filter((u) => !entryFor(subject, u));
  const toRemove = units.filter(
    (u) => ocIndexOf(u) > tail && entryFor(subject, u) && !desired.has(u.id)
  );

  if (toAdd.length) {
    overcompletions.value.push(
      ...toAdd.map((u) => ({ subject, chapter_reached: u.title, node_id: u.id }))
    );
  }
  for (const u of toRemove) {
    removeOcOf(subject, u);
    if (u.level === "chapter") {
      for (const k of childrenOfChapter(u.id)) removeOcOf(subject, k);
    }
  }
  // 整章推进后章内知识点已被章条目覆盖，清掉本轮零散的知识点条目避免冗余
  for (const u of toAdd) {
    if (u.level === "chapter") {
      for (const k of childrenOfChapter(u.id)) removeOcOf(subject, k);
    }
  }

  if (toAdd.length || toRemove.length) {
    await persistActiveTable(subject, (copy) => {
      for (const u of toAdd) applyNodeState(subject, table.id, copy, u.id, true);
      for (const u of toRemove) applyNodeState(subject, table.id, copy, u.id, false);
    });
  }
  ocHint.value = `已将${subjectLabel(subject)}进度推进到「${target.title}」，该章及此前未记录的内容已整段记为本次计划外。`;
  await nextTick(() => scrollToUnit(target.id));
}

/** 回退确认：把该科进度基准回退到完成区内所选章节，其后内容恢复未学 */
async function confirmCorrection() {
  const node = correctionTarget.value;
  if (!node) return;
  const subject = activeSubject.value;
  const table = activeTable.value;
  if (!table) return;
  ensureOcSnapshot(subject);
  const idx = ocIndexOf(node);
  const tail = ocTail.value[`${subject}|${table.id}`] ?? -1;
  if (idx < 0 || idx >= tail) {
    correctionTarget.value = null;
    return;
  }
  const affected = ocUnits.value.slice(idx + 1, tail + 1);
  for (const u of affected) {
    removeOcOf(subject, u);
    if (u.level === "chapter") {
      for (const k of childrenOfChapter(u.id)) removeOcOf(subject, k);
    }
  }
  await persistActiveTable(subject, (copy) => {
    for (const u of affected) {
      const n = copy.nodes.find((x) => x.id === u.id);
      if (!n) continue;
      n.status = "pending";
      if (n.level === "chapter") {
        for (const k of copy.nodes) {
          if (k.parent_id === n.id) k.status = "pending";
        }
      }
    }
  });
  ocTail.value[`${subject}|${table.id}`] = idx;
  ocHint.value = `已将${subjectLabel(subject)}的进度基准回退到「${node.title}」，其后内容恢复为未学；后续计划以本次复盘的重排结果为准。`;
  correctionTarget.value = null;
}

function cancelCorrection() {
  correctionTarget.value = null;
}

/** 章节行点击入口：完成区之前 → 确认回退；完成区尾部 → 提示；之后 → 前向推进 */
async function onUnitClick(node: ProgressNode) {
  if (savingProgress.value) return;
  const idx = ocIndexOf(node);
  const tail = ocTailIdx.value;
  if (idx < 0) return;
  if (idx < tail) {
    correctionTarget.value = node;
    ocHint.value = "";
    return;
  }
  if (idx === tail) {
    ocHint.value = `「${node.title}」正是当前记录进度（已学至此处）；若你本次实际已学到它之后的内容，请点选更靠后的章节。`;
    return;
  }
  await applyForwardPointer(idx);
}

/** 行提示文案：说明已学区 / 目前进度 / 前向推进的语义 */
function pointerRowHint(ui: number): string {
  const tail = ocTailIdx.value;
  if (ui < tail) {
    return "系统记录中已学过的章节；点选表示实际进度没有到达记录位置（记录超前），确认后可回退进度";
  }
  if (ui === tail) {
    return "目前进度：已学至此处；若本次实际学得更靠后，请点选其后的章节";
  }
  return "点选 = 本次实际进度到达这里，从目前进度到该章之间未记录的内容将整段补记";
}

/** 已展开的章节 id（章节可展开，逐条勾选其下具体知识点） */
const ocExpanded = ref<Set<string>>(new Set());
function isOcExpanded(chapterId: string): boolean {
  return ocExpanded.value.has(chapterId);
}
function toggleOcExpand(chapterId: string) {
  const next = new Set(ocExpanded.value);
  if (next.has(chapterId)) next.delete(chapterId);
  else next.add(chapterId);
  ocExpanded.value = next;
}

/** 章节内知识点统计：状态 ≥ 基础 或 本轮已勾选 记入已学 */
function chapterKidStats(chapterId: string): { learned: number; total: number } {
  const kids = childrenOfChapter(chapterId);
  let learned = 0;
  for (const k of kids) {
    if (ocIsLearned(k) || isChecked(activeSubject.value, k)) learned += 1;
  }
  return { learned, total: kids.length };
}

/**
 * 单个知识点的勾选：只推进该知识点自身（状态 → 基础）并记入本次计划外，
 * 不改变章节顺序进度指针 —— 用于「只学到本章中间几个知识点」的精确声明。
 * 取消勾选时把该节点状态还原到进入本轮会话时的快照。
 */
async function toggleKnowledgePoint(subject: string, node: ProgressNode) {
  if (savingProgress.value) return;
  const table = viewTableOf(subject);
  if (!table) return;
  ensureOcSnapshot(subject);
  const existing = entryFor(subject, node);
  if (existing) {
    overcompletions.value = overcompletions.value.filter((oc) => oc !== existing);
    await persistActiveTable(subject, (t) => {
      const n = t.nodes.find((x) => x.id === node.id);
      if (n) n.status = ocSnapshotStatus(subject, table.id, n.id);
    });
    ocHint.value = `已撤销本次对「${node.title}」的计划外记录。`;
    return;
  }
  overcompletions.value.push({ subject, chapter_reached: node.title, node_id: node.id });
  await persistActiveTable(subject, (t) => {
    const n = t.nodes.find((x) => x.id === node.id);
    if (n && ocRankOf(n) < STATUS_RANK.basic) n.status = "basic";
  });
  ocHint.value = `已将「${node.title}」记为本次计划外学习（状态推进到「基础」）。`;
}

/** 自动定位：把进度锚点后的首个待学章节滚入视野 */
function scrollToUnit(id: string, behavior: ScrollBehavior = "smooth") {
  ocListRef.value
    ?.querySelector(`[data-uid="${id}"]`)
    ?.scrollIntoView({ block: "nearest", behavior });
}
function scrollToCurrentProgress() {
  const u = ocNextUnit.value ?? ocTailUnit.value;
  if (u) scrollToUnit(u.id);
}

// 进入第 6 步 / 切换科目或进度表时：锁定快照并把进度锚点滚入视野
watch(
  [step, activeSubject, () => activeTable.value?.id, hasOvercompletion],
  () => {
    if (step.value === 4 && hasOvercompletion.value && activeTable.value) {
      ensureOcSnapshot(activeSubject.value);
      nextTick(scrollToCurrentProgress);
    }
  },
  { flush: "post" }
);

function newNodeId(): string {
  return `n-${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
}

/** 新建节点：有章节→建章节节点；无章节→建知识点节点；创建后落盘并勾选（状态按「基础」起步） */
async function createChapter(subject: string) {
  const title = newChapterTitle.value.trim();
  if (!title) return;
  newChapterError.value = "";
  if ((activeTable.value?.nodes ?? []).some((n) => n.title.trim() === title)) {
    newChapterError.value = "该章节已存在";
    return;
  }
  const node: ProgressNode = {
    id: newNodeId(),
    title,
    level: hasChapters.value ? "chapter" : "knowledge",
    parent_id: null,
    phase: hasChapters.value ? title : "",
    status: "basic",
    planned_date: null,
    note: "",
  };
  await persistActiveTable(subject, (t) => {
    t.nodes.push(node);
  });
  overcompletions.value.push({ subject, chapter_reached: node.title, node_id: node.id });
  newChapterTitle.value = "";
}

function subjectBadgeVariant(s: string): "math" | "english" | "politics" | "professional" | "default" {
  const set = new Set(["math", "english", "politics", "professional"]);
  return set.has(s) ? (s as any) : "default";
}

function statusLabel(s: string): string {
  const m: Record<string, string> = { completed: "已完成", partial: "部分完成", incomplete: "未完成", abandoned: "放弃" };
  return m[s] ?? s;
}

function feelingLabel(s: string): string {
  const m: Record<string, string> = { smooth: "😊 很顺利", normal: "😐 一般", hard: "😣 比较困难" };
  return m[s] ?? s;
}

function difficultyLabel(s: string): string {
  const m: Record<string, string> = {
    understanding: "理解概念",
    problems: "做题",
    memorization: "记忆",
    attention: "注意力",
    time_management: "时间安排",
    environment: "学习环境",
    other: "其它",
  };
  return m[s] ?? (s || "—");
}

function toggleBlocker(taskId: string, value: string) {
  const current = taskBlockers.value[taskId] ?? [];
  if (current.includes(value)) {
    taskBlockers.value[taskId] = current.filter(v => v !== value);
  } else {
    taskBlockers.value[taskId] = [...current, value];
  }
}

// ── Init from state ──
function initFromState() {
  if (!plan.value) return;
  for (const task of allTasks.value) {
    taskCompleted.value[task.id] = task.status === "done";
  }
}

/** 回读滴答清单当日已完成任务，把手机端勾选完成的任务在复盘页自动标记为完成 */
async function loadDidaCompleted() {
  const target = selectedDate.value;
  didaMatchedCount.value = 0;
  try {
    const titles = await api.fetchDidaCompletedTitles(target);
    // 回读期间用户已切换日期：丢弃本次结果，避免勾选串到别的日期
    if (selectedDate.value !== target) return;
    if (!titles.length) return;
    let matched = 0;
    for (const task of allTasks.value) {
      if (titles.includes(task.title)) {
        taskCompleted.value[task.id] = true;
        matched++;
      }
    }
    didaMatchedCount.value = matched;
  } catch {
    // 未启用同步 / 未配置 Token / 网络异常等场景静默忽略，不阻塞复盘
  }
}

// 从已有复盘初始化（用于查看模式展示，兼容旧版与新版）
function initFromReview(review: ReviewRecord) {
  // 新版：优先使用 task_reviews
  if (review.task_reviews?.length) {
    for (const tr of review.task_reviews) {
      taskCompleted.value[tr.task_id] = tr.status === "completed";
      if (tr.blockers?.length) taskBlockers.value[tr.task_id] = [...tr.blockers];
      if (tr.blocker_note) blockerNotes.value[tr.task_id] = tr.blocker_note;
    }
  } else if (review.data?.completed_tasks?.length) {
    // 旧版：从 data.completed_tasks 回填完成状态
    for (const ct of review.data.completed_tasks) {
      const tid = ct.task_id ?? ct.title;
      if (tid) taskCompleted.value[tid] = ct.completed;
    }
  }
  if (review.daily_review) {
    overallFeeling.value = review.daily_review.overall_feeling || "normal";
    mainDifficulty.value = review.daily_review.main_difficulty || "";
    workloadFeedback.value = review.daily_review.workload_feedback || "reasonable";
    externalInterference.value = review.daily_review.external_interference || "none";
  }
  // 计划外学习记录（用于勾选回显 + 只读展示）
  if (review.overcompletion?.length) {
    hasOvercompletion.value = true;
    overcompletions.value = review.overcompletion.map(oc => ({ ...oc }));
    // 默认定位到有进度表的科目（优先已记录科目的科目），便于直接回显勾选态
    activeSubject.value =
      review.overcompletion.find(oc => hasTable(oc.subject))?.subject
      ?? OC_SUBJECTS.find(hasTable)
      ?? "math";
  }
}

function resetForm() {
  step.value = 0;
  quickMode.value = false;
  taskCompleted.value = {};
  taskBlockers.value = {};
  blockerNotes.value = {};
  overallFeeling.value = "normal";
  mainDifficulty.value = "";
  workloadFeedback.value = "reasonable";
  externalInterference.value = "none";
  hasOvercompletion.value = false;
  overcompletions.value = [];
  progressIndex.value = null;
  activeSubject.value = "math";
  viewTableIds.value = {};
  correctionTarget.value = null;
  ocHint.value = "";
  ocOrig.value = {};
  ocTail.value = {};
}

// ── Navigation ──
// 步骤顺序：0 完成情况 → 1 未完成原因（可跳过）→ 2 整体感受 → 3 最大困难 → 4 计划外学习
function canNext(): boolean {
  switch (step.value) {
    case 0: return allTasks.value.length > 0;
    case 2: return !!overallFeeling.value;
    default: return true;
  }
}

function startQuickReview() {
  quickMode.value = true;
  step.value = 2;
}

function goNext() {
  if (step.value >= totalSteps - 1) return;
  // 若所有任务均已完成，则跳过「未完成原因」步骤
  if (step.value === 0) {
    step.value = incompleteTasks.value.length === 0 ? 2 : 1;
    return;
  }
  if (step.value === 1) {
    step.value = 2;
    return;
  }
  step.value++;
}
function goPrev() {
  if (quickMode.value && step.value === 2) {
    quickMode.value = false;
    step.value = 0;
    return;
  }
  if (step.value <= 0) return;
  if (step.value === 2) {
    // 若所有任务均已完成，则跳过「未完成原因」步骤
    step.value = incompleteTasks.value.length === 0 ? 0 : 1;
    return;
  }
  step.value--;
}

// ── Submit ──
async function doSubmit() {
  submitting.value = true;
  // 动态步骤：保存复盘 →（可能）调整后续计划 → 同步滴答
  submitSteps.value = buildSubmitSteps(DEFAULT_SUBMIT_STEP_KEYS);
  setSubmitStep(0); // 正在保存复盘
  try {
    const taskReviews: TaskReviewEntry[] = allTasks.value.map(t => ({
      task_id: t.id,
      status: taskCompleted.value[t.id] ? "completed" : "incomplete",
      completion: taskCompleted.value[t.id] ? 1.0 : 0.0,
      // 掌握程度不再由复盘采集（该职责已由进度表承担）：字段留空，兼容历史记录
      mastery: "",
      blockers: taskBlockers.value[t.id] || [],
      blocker_note: blockerNotes.value[t.id] || undefined,
      title: t.title,
      subject: t.subject,
      priority: t.priority,
      // 仅在启用「记录学习时长」时持久化估时与实际用时
      estimated_hours: timeTrackingEnabled.value && t.estimated_hours > 0 ? t.estimated_hours : undefined,
      actual_minutes: timeTrackingEnabled.value ? (taskActualMinutes.value[t.id] ?? 0) : undefined,
    }));

    const dailyReview: DailyReviewInput = {
      overall_feeling: overallFeeling.value,
      main_difficulty: mainDifficulty.value,
      workload_feedback: workloadFeedback.value,
      external_interference: externalInterference.value,
    };

    // 仅在用户开启计划外学习且填写了有效章节时提交
    const validOvercompletions = hasOvercompletion.value
      ? overcompletions.value.filter(oc => oc.subject && oc.chapter_reached.trim())
      : [];

    const result = await api.submitReview({
      date: selectedDate.value,
      task_reviews: taskReviews,
      daily_review: dailyReview,
      overcompletion: validOvercompletions.length > 0 ? validOvercompletions : undefined,
    });
    submitted.value = true;

    // 若需要 AI 重排剩余天数，插入「正在调整」步骤并执行
    if (result.needs_regeneration) {
      await advanceSubmitStep(ensureSubmitStep("adjusting"));
      await executeRegeneration();
    }

    // 收尾：清理滴答中过往未完成任务
    await advanceSubmitStep(ensureSubmitStep("syncing"));
    try {
      await api.cleanupDidaStale();
    } catch (e) {
      console.error("清理滴答过往任务失败:", e);
    }
    // 全部完成，关闭进度卡片
    setSubmitStep(-1);

    // 重新加载复盘
    await loadReviewData();
    // 刷新复盘日期列表
    await loadReviewDates();
  } catch (e) {
    console.error("提交复盘失败:", e);
  } finally {
    submitting.value = false;
    setSubmitStep(-1);
  }
}

/** 取消 AI 重排剩余天数（M9：超时过长且无取消机制） */
async function cancelRegeneration() {
  try {
    const found = await api.cancelAiRequest(api.AI_CANCEL_KEYS.planner);
    regenMessage.value = found
      ? "正在取消 AI 调整，请稍候…"
      : "未找到进行中的 AI 调整请求";
  } catch {
    regenMessage.value = "取消失败，请稍后再试";
  }
}

/** 执行 AI 重排剩余天数（doSubmit 和 retry 共用） */
async function executeRegeneration() {
  // 已在「正在调整」步骤时不重复切换：否则会重置该步的最短展示计时
  if (submitSteps.value[submitStepIndex.value]?.key !== "adjusting") {
    setSubmitStep(ensureSubmitStep("adjusting")); // 正在调整
  }
  regenFailed.value = false;
  regenerating.value = true;
  regenMessage.value = "正在调整后续计划，请勿关闭应用…";
  saveRegenStatus({
    date: selectedDate.value,
    regenerating: true,
    message: regenMessage.value,
    failed: false,
    timestamp: Date.now(),
    changes: regenChanges.value,
  });
  try {
    const regenResult = await api.regenerateRemainingDays(selectedDate.value);
    if (regenResult.used_fallback) {
      // AI 调用失败，但兜底安排了未完成任务：告知用户并提供重新生成按钮
      regenFailed.value = true;
      regenMessage.value =
        "AI 调整后续计划失败，已自动按未完成任务做了兜底安排。可点击「重新生成」再次尝试让 AI 调整。";
    } else if (regenResult.regenerated) {
      regenMessage.value = `已调整后续 ${regenResult.affected_dates.length} 天的计划安排`;
    } else {
      regenMessage.value = "本次复盘无需调整后续计划。";
    }
    // 保存逐日变更明细（悬停查看具体修改了哪些内容）
    regenChanges.value = regenResult.changes ?? [];
    // 一致性校验：声明了计划外进度的科目未在计划中生效时，追加警告提示
    if (regenResult.consistency_warnings?.length) {
      regenMessage.value += `\n${regenResult.consistency_warnings.join("\n")}`;
      regenFailed.value = true;
    }
  } catch (e) {
    console.error("调整后续计划失败:", e);
    const detail = e instanceof Error ? e.message : String(e);
    regenMessage.value = `调整失败：${detail}（不影响复盘结果）`;
    regenFailed.value = true;
    regenChanges.value = [];
  } finally {
    regenerating.value = false;
    if (regenMessage.value) {
      saveRegenStatus({
        date: selectedDate.value,
        regenerating: false,
        message: regenMessage.value,
        failed: regenFailed.value,
        timestamp: Date.now(),
        changes: regenChanges.value,
      });
    } else {
      clearRegenStatus();
    }
  }
}

/** 重试 AI 重排剩余天数 */
async function retryRegeneration() {
  // 重试只跑「正在调整」一步：重置步骤列表，避免展示与本次无关的步骤
  submitSteps.value = buildSubmitSteps(["adjusting"]);
  submitStepIndex.value = -1;
  await executeRegeneration();
  // 关闭进度卡片，回到复盘结果页（无论成败都回到页面展示提示/重试按钮）
  setSubmitStep(-1);
  // 重排成功后重新加载复盘数据以同步最新计划
  if (!regenFailed.value) {
    await loadReviewData();
    await loadReviewDates();
  }
}

// ── Data loading ──
async function loadTaskActualMinutes() {
  if (!timeTrackingEnabled.value) {
    taskActualMinutes.value = {};
    return;
  }
  try {
    const map: Record<string, number> = {};
    if (selectedDate.value === todayDate) {
      const state = await api.getState();
      for (const st of state.current_task?.tasks ?? []) {
        if (!st.task_id) continue;
        let total = st.accumulated_minutes ?? 0;
        // 若任务正在计时，加上当前进行中的时段
        if (st.started_at) {
          const start = new Date(st.started_at).getTime();
          const now = Date.now();
          if (!isNaN(start) && now > start) {
            total += Math.floor((now - start) / 60000);
          }
        }
        map[st.task_id] = total;
      }
    } else {
      // State 只保留当前日；历史日从带 task_id 的 Focus 会话恢复任务用时。
      const sessions = await api.getFocusSessions(selectedDate.value);
      for (const session of sessions) {
        if (session.status !== "completed" || !session.task_id) continue;
        if (session.type !== "focus" && session.type !== "stopwatch") continue;
        map[session.task_id] = (map[session.task_id] ?? 0) + Math.max(0, session.duration_minutes);
      }
    }
    taskActualMinutes.value = map;
  } catch {
    taskActualMinutes.value = {};
  }
}

async function loadReviewData() {
  loading.value = true;
  resetForm();
  existingReview.value = null;
  submitted.value = false;

  // 重排提示：会话内持久化（sessionStorage），切换日期不清除、切回仍有；
  // 仅当持久化记录属于当前日期时才展示，避免在别的日期误显示陈旧的调整提示
  const persistedRegen = loadRegenStatus();
  if (persistedRegen && persistedRegen.date === selectedDate.value) {
    regenerating.value = persistedRegen.regenerating;
    regenMessage.value = persistedRegen.message;
    regenFailed.value = persistedRegen.failed;
    regenChanges.value = persistedRegen.changes ?? [];
    // 切页回来时 AI 仍在调整：恢复进度卡片到「正在调整」步骤
    if (persistedRegen.regenerating) {
      setSubmitStep(ensureSubmitStep("adjusting"));
    } else {
      setSubmitStep(-1);
    }
  } else {
    regenerating.value = false;
    regenMessage.value = "";
    regenFailed.value = false;
    regenChanges.value = [];
    setSubmitStep(-1);
  }

  try {
    // 加载选中日期的计划（用于显示任务列表）
    await todayStore.loadByDate(selectedDate.value);
    // 加载各科进度表（计划外学习章节选择数据源）
    await loadProgressIndex();
    // 加载任务实际用时（仅启用计时且当前日匹配时有效）
    await loadTaskActualMinutes();
    // 检查选中日期是否为排除日
    await checkSelectedDateExcluded();

    // 检查是否已有复盘
    try {
      existingReview.value = await api.getReview(selectedDate.value);
      if (existingReview.value) {
        submitted.value = true;
        // 从已有复盘初始化表单数据（用于只读展示）
        initFromReview(existingReview.value);
      }
    } catch {
      // 无已有复盘
    }

    // 如果不是只读模式，从 state 初始化任务完成状态；
    // 仅当该日期可填写复盘（今天已过结束时间 / 昨天补复盘窗口）时才回读滴答，
    // 且不阻塞加载（异步应用勾选结果）
    if (!submitted.value) {
      initFromState();
      if (canFillReview.value) {
        loadDidaCompleted();
      }
    }
  } finally {
    loading.value = false;
  }
}

async function loadReviewDates() {
  try {
    reviewDates.value = await api.listReviewDates();
  } catch {
    reviewDates.value = [];
  }
}

// 跳转到指定复盘日期
function jumpToReviewDate(date: string) {
  // 根据目标日期与当前日期的关系决定方向
  const dir = date < selectedDate.value ? "prev" : "next";
  setDate(date, dir);
}

// ── Watch date changes ──
watch(selectedDate, () => {
  loadReviewData();
});

// 从 URL query 初始化日期
function initDateFromQuery() {
  const q = route.query.date;
  if (typeof q === "string" && /^\d{4}-\d{2}-\d{2}$/.test(q)) {
    selectedDate.value = q;
  } else if (beforeStartTimeToday.value) {
    // 次日凌晨（今天学习开始前）打开复盘：默认进入前一天日期进行复盘。
    // 复盘对象是「刚结束的那一天」，此时今天的学习尚未开始，不应默认到第二天。
    selectedDate.value = yesterdayDate.value;
  }
}

onMounted(async () => {
  initDateFromQuery();
  await Promise.all([loadReviewData(), loadReviewDates()]);
});

// 历史复盘日期下拉
const showHistoryDropdown = ref(false);
const sortedReviewDates = computed(() => [...reviewDates.value].reverse());
</script>

<template>
  <div class="review-view">
    <!-- Date navigation bar -->
    <div class="date-bar">
      <div class="date-nav">
        <Button
          variant="ghost"
          size="sm"
          :disabled="selectedDate <= '2020-01-01'"
          @click="goPrevDay"
        >
          <ChevronLeft :size="16" />
        </Button>
        <div class="date-display">
          <Calendar :size="14" />
          <span class="date-text">{{ selectedDate }}</span>
          <span v-if="isToday" class="date-tag today-tag">今天</span>
          <span v-else-if="isYesterday" class="date-tag yesterday-tag">昨天</span>
          <span v-else-if="isFuture" class="date-tag future-tag">未来</span>
          <span v-else class="date-tag past-tag">历史</span>
        </div>
        <Button
          variant="ghost"
          size="sm"
          :disabled="isToday"
          @click="goNextDay"
        >
          <ChevronRight :size="16" />
        </Button>
      </div>
      <div class="date-actions">
        <!-- 历史复盘下拉 -->
        <div class="history-dropdown-wrapper">
          <Button
            variant="ghost"
            size="sm"
            @click="showHistoryDropdown = !showHistoryDropdown"
          >
            <History :size="14" />
            历史复盘
          </Button>
          <div v-if="showHistoryDropdown" class="history-dropdown" @click.stop>
            <div class="dropdown-header">选择日期查看复盘</div>
            <div v-if="sortedReviewDates.length === 0" class="dropdown-empty">
              暂无复盘记录
            </div>
            <button
              v-for="date in sortedReviewDates"
              :key="date"
              type="button"
              class="dropdown-item"
              :class="{ active: date === selectedDate }"
              @click="jumpToReviewDate(date); showHistoryDropdown = false"
            >
              {{ date }}
              <span v-if="date === todayDate" class="item-tag">今天</span>
              <span v-else-if="date === yesterdayDate" class="item-tag">昨天</span>
            </button>
          </div>
        </div>
        <Button v-if="!isToday" variant="ghost" size="sm" @click="goToday">
          回到今天
        </Button>
      </div>
    </div>

    <!-- Loading -->
    <transition :name="dateTransitionName" mode="out-in">
    <div :key="selectedDate" class="review-content">
    <div v-if="loading" class="loading-msg">加载中…</div>

    <!-- Future date -->
    <Card v-else-if="isFuture" padding="lg" class="gate-card">
      <div class="gate-hero">
        <div class="gate-icon"><Calendar :size="40" /></div>
        <h1 class="gate-title">未来日期</h1>
        <p class="gate-desc">无法为未来日期创建复盘。</p>
        <Button variant="primary" size="sm" @click="goToday">回到今天</Button>
      </div>
    </Card>

    <!-- Today: before end time -->
    <Card v-else-if="isToday && beforeEndTime" padding="lg" class="gate-card">
      <div class="gate-hero">
        <div class="gate-icon"><Clock :size="40" /></div>
        <h1 class="gate-title">今日尚未结束</h1>
        <p class="gate-desc">每日复盘需在 {{ endTime }} 之后进行。</p>
        <p class="gate-hint">请在学习结束后再来复盘。</p>
      </div>
    </Card>

    <!-- 休息日：无需复盘 -->
    <Card v-else-if="isCurrentDateRestDay && !existingReview" padding="lg" class="gate-card">
      <div class="gate-hero">
        <div class="gate-icon"><Coffee :size="40" /></div>
        <h1 class="gate-title">{{ selectedDate }} 是休息日</h1>
        <p class="gate-desc">休息日无需复盘，好好放松一下吧。</p>
        <Button v-if="!isToday" variant="secondary" size="sm" @click="goToday">回到今天</Button>
      </div>
    </Card>

    <!-- 排除日：无需复盘 -->
    <Card v-else-if="selectedDateExcluded && !existingReview" padding="lg" class="gate-card">
      <div class="gate-hero">
        <div class="gate-icon"><Ban :size="40" /></div>
        <h1 class="gate-title">{{ selectedDate }} 是排除日</h1>
        <p class="gate-desc">{{ selectedDateExcludedReason || '特殊情况排除日，无需复盘。' }}</p>
        <Button v-if="!isToday" variant="secondary" size="sm" @click="goToday">回到今天</Button>
      </div>
    </Card>

    <!-- No plan -->
    <EmptyState
      v-else-if="!plan"
      :title="`${selectedDate} 没有学习计划`"
      :description="isToday ? '今天还没有学习计划：周计划已内置，可在「今日计划」页一键生成，日计划会自动拆分' : '该日无学习计划，无法复盘'"
    >
      <template #actions>
        <Button v-if="isToday" variant="primary" @click="router.push({ name: 'plan' })">
          去今日计划生成
        </Button>
        <Button v-if="!isToday" variant="secondary" @click="goToday">回到今天</Button>
      </template>
    </EmptyState>

    <!-- Yesterday without review and past start_time (cannot backfill) -->
    <Card v-else-if="isYesterday && !existingReview && !beforeStartTimeToday" padding="lg" class="gate-card">
      <div class="gate-hero">
        <div class="gate-icon"><AlertTriangle :size="40" /></div>
        <h1 class="gate-title">无法补复盘</h1>
        <p class="gate-desc">昨天未进行复盘，但今日学习已开始。</p>
        <p class="gate-hint">补复盘仅可在今日学习开始时间（{{ startTime }}）之前进行。</p>
      </div>
    </Card>

    <!-- Older than yesterday without review -->
    <Card v-else-if="isOlderThanYesterday && !existingReview" padding="lg" class="gate-card">
      <div class="gate-hero">
        <div class="gate-icon"><Calendar :size="40" /></div>
        <h1 class="gate-title">无复盘记录</h1>
        <p class="gate-desc">{{ selectedDate }} 没有复盘记录。</p>
        <p class="gate-hint">仅可对昨天补复盘，更早的日期无法补录。</p>
      </div>
    </Card>

    <!-- Submitting / AI regenerating -->
    <!-- 置于「已提交」分支之前：复盘已保存但 AI 仍在调整时，切页回来也优先显示调整中页面 -->
    <Card v-if="submitStepIndex >= 0 || regenerating" padding="lg" class="gate-card">
      <div class="gate-hero">
        <div class="gate-icon"><LoadingSpinner :size="40" /></div>
        <h1 class="gate-title">{{ regenerating ? '正在调整后续计划…' : '正在提交复盘…' }}</h1>
        <p class="gate-desc">请稍候，正在处理你的学习数据。</p>
        <div class="submit-steps">
          <div
            v-for="(s, i) in submitSteps"
            :key="s.key"
            class="submit-step"
            :class="{ done: i < submitStepIndex, active: i === submitStepIndex, pending: i > submitStepIndex }"
          >
            <span class="submit-step-icon">
              <CheckCircle2 v-if="i < submitStepIndex" :size="16" />
              <LoadingSpinner v-else-if="i === submitStepIndex" :size="16" />
              <span v-else class="submit-step-dot"></span>
            </span>
            <span class="submit-step-label">{{ s.label }}</span>
          </div>
        </div>
        <Button v-if="regenerating" variant="ghost" size="sm" class="gate-cancel-btn" @click="cancelRegeneration">
          取消本次调整
        </Button>
      </div>
    </Card>

    <!-- Already submitted / read-only review -->
    <template v-else-if="submitted && existingReview">
      <Card padding="lg" class="done-card">
        <div class="done-hero">
          <div class="done-badge"><Check :size="32" /></div>
          <h1 class="done-title">
            {{ isToday ? '今日复盘已完成' : `${selectedDate} 复盘记录` }}
          </h1>
          <p class="done-desc">{{ isToday ? '今天的结构化复盘已保存。' : '查看历史复盘记录。' }}</p>

          <!-- 重排提示 -->
          <div v-if="regenMessage" class="regen-banner" :class="{ 'regen-loading': regenerating, 'regen-error': regenFailed }">
            <AlertTriangle :size="18" v-if="regenerating" />
            <CheckCircle2 :size="18" v-else-if="!regenFailed" />
            <AlertTriangle :size="18" v-else />
            <span>{{ regenMessage }}</span>
            <!-- 变更明细：常态化展示调整提示，鼠标悬停查看具体修改了哪些内容 -->
            <div
              v-if="regenChanges.length && !regenerating"
              class="regen-details-trigger"
            >
              <Info :size="13" />
              <span>悬停查看变更</span>
              <div class="regen-details-popover">
                <div class="regen-details-inner">
                  <div v-if="regenFailed" class="regen-detail-fallback">
                    本次为兜底安排（按未完成任务程序化分配），各日任务如下：
                  </div>
                  <div v-for="c in regenChanges" :key="c.date" class="regen-detail-day">
                    <div class="regen-detail-date">{{ c.date }}</div>
                    <div v-for="t in c.added" :key="'a' + t" class="regen-detail-item add">
                      ＋{{ t }}
                    </div>
                    <div v-for="r in c.removed" :key="'r' + r" class="regen-detail-item remove">
                      －{{ r }}
                    </div>
                    <div v-for="(adj, i) in c.adjusted" :key="'m' + i" class="regen-detail-item modify">
                      ✎ {{ adj[0] }} → {{ adj[1] }}
                    </div>
                    <div
                      v-if="!c.added.length && !c.removed.length && !c.adjusted.length"
                      class="regen-detail-item none"
                    >
                      任务量分配微调，无标题变动
                    </div>
                  </div>
                </div>
              </div>
            </div>
            <Button v-if="regenFailed" variant="primary" size="sm" @click="retryRegeneration" :loading="regenerating" class="regen-retry-btn">
              重新生成
            </Button>
          </div>

          <!-- Review summary -->
          <div v-if="existingReview.task_reviews?.length || existingReview.data?.completed_tasks?.length" class="review-summary">
            <div class="summary-row">
              <span class="summary-label">完成率</span>
              <span class="summary-value">
                <template v-if="existingReview.task_reviews?.length">
                  {{ existingReview.task_reviews.filter(t => t.status === 'completed').length }} / {{ existingReview.task_reviews.length }}
                </template>
                <template v-else>
                  {{ existingReview.data.completed_tasks.filter(t => t.completed).length }} / {{ existingReview.data.completed_tasks.length }}
                </template>
              </span>
            </div>
            <div v-if="existingReview.data?.total_hours" class="summary-row">
              <span class="summary-label">学习时长</span>
              <span class="summary-value">{{ existingReview.data.total_hours.toFixed(1) }} 小时</span>
            </div>
            <div v-if="existingReview.daily_review?.overall_feeling" class="summary-row">
              <span class="summary-label">整体感受</span>
              <span class="summary-value">{{ feelingLabel(existingReview.daily_review.overall_feeling) }}</span>
            </div>
            <div v-if="existingReview.daily_review?.main_difficulty" class="summary-row">
              <span class="summary-label">最大困难</span>
              <span class="summary-value">{{ difficultyLabel(existingReview.daily_review.main_difficulty) }}</span>
            </div>
          </div>

          <!-- Task-level review details (new version) -->
          <div v-if="existingReview.task_reviews?.length" class="task-reviews-list">
            <div class="task-reviews-title">任务复盘详情</div>
            <div
              v-for="tr in existingReview.task_reviews"
              :key="tr.task_id"
              class="task-review-row"
              :class="tr.status"
            >
              <div class="trr-left">
                <div class="trr-status-icon">
                  <CheckCircle2 v-if="tr.status === 'completed'" :size="16" />
                  <Circle v-else :size="16" />
                </div>
                <div class="trr-title-wrap">
                  <span class="trr-title">{{ findTaskTitle(tr.task_id, tr) }}</span>
                  <div class="trr-meta">
                    <span v-if="findTaskSubject(tr.task_id, tr)" class="trr-subject">{{ subjectLabel(findTaskSubject(tr.task_id, tr)) }}</span>
                    <span class="trr-status">{{ statusLabel(tr.status) }}</span>
                  </div>
                </div>
              </div>
              <div class="trr-right">
                <span v-if="timeTrackingEnabled && taskEstimatedHours(tr.task_id, tr) > 0" class="trr-chip estimate">
                  <Clock :size="11" />
                  估时 {{ formatHours(taskEstimatedHours(tr.task_id, tr)) }}
                </span>
                <span v-if="timeTrackingEnabled && taskActualFromReview(tr.task_id, tr) > 0" class="trr-chip actual">
                  <Clock :size="11" />
                  实际 {{ formatMinutes(taskActualFromReview(tr.task_id, tr)) }}
                </span>
                <span v-if="tr.mastery" class="trr-chip mastery">
                  {{ tr.mastery === 'mastered' ? '已掌握' : tr.mastery === 'basic' ? '基本掌握' : '需巩固' }}
                </span>
                <span v-for="b in tr.blockers" :key="b" class="trr-chip blocker">
                  {{ blockerOptions.find(o => o.value === b)?.label ?? b }}
                </span>
                <span v-if="tr.blocker_note" class="trr-note">{{ tr.blocker_note }}</span>
              </div>
            </div>
          </div>

          <!-- Old version: completed_tasks fallback -->
          <div v-else-if="existingReview.data?.completed_tasks?.length" class="task-reviews-list">
            <div class="task-reviews-title">任务完成情况（旧版记录）</div>
            <div
              v-for="ct in existingReview.data.completed_tasks"
              :key="ct.task_id ?? ct.title"
              class="task-review-row"
              :class="ct.completed ? 'completed' : 'incomplete'"
            >
              <div class="trr-left">
                <div class="trr-status-icon">
                  <CheckCircle2 v-if="ct.completed" :size="16" />
                  <Circle v-else :size="16" />
                </div>
                <div class="trr-title-wrap">
                  <span class="trr-title">{{ ct.title }}</span>
                  <div class="trr-meta">
                    <span class="trr-subject">{{ subjectLabel(ct.subject) }}</span>
                    <span class="trr-status">{{ ct.completed ? '已完成' : '未完成' }}</span>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <!-- Overcompletion records -->
          <div v-if="existingReview.overcompletion?.length" class="task-reviews-list">
            <div class="task-reviews-title">计划外学习内容</div>
            <div
              v-for="(oc, idx) in existingReview.overcompletion"
              :key="idx"
              class="task-review-row overcompletion"
            >
              <div class="trr-left">
                <div class="trr-status-icon"><Sparkles :size="16" /></div>
                <div class="trr-title-wrap">
                  <span class="trr-title">{{ subjectLabel(oc.subject) }}：{{ oc.chapter_reached }}</span>
                  <div class="trr-meta">
                    <span class="trr-subject">{{ subjectLabel(oc.subject) }}</span>
                    <span class="trr-status">实际进度</span>
                  </div>
                </div>
              </div>
              <div class="trr-right">
                <span v-if="oc.note" class="trr-note">{{ oc.note }}</span>
              </div>
            </div>
          </div>

          <p class="done-hint">复盘数据将在下一次 Planner 中自动生效。</p>
        </div>
      </Card>
    </template>

    <!-- Steps (fill review) -->
    <template v-else-if="canFillReview">
      <div class="backfill-banner" v-if="isYesterday">
        <AlertTriangle :size="14" />
        <span>补复盘模式：为昨天（{{ selectedDate }}）补录复盘</span>
      </div>

      <div class="step-bar">
        <div class="step-dots">
          <span v-for="i in totalSteps" :key="i" class="step-dot"
            :class="{ active: i - 1 === step, done: i - 1 < step }" />
        </div>
        <span class="step-label">{{ step + 1 }} / {{ totalSteps }}</span>
        <Button v-if="step === 0" variant="ghost" size="sm"
          :disabled="doneTasks.length === 0"
          title="请先勾选已完成的任务（滴答清单同步的勾选同样生效）后再使用快速复盘"
          @click="startQuickReview">
          <Clock :size="14" /> 快速复盘（约 30 秒）
        </Button>
      </div>

      <!-- Step 1: Task Completion -->
      <Card v-if="step === 0" padding="lg" class="step-card">
        <h2 class="step-title">任务完成情况</h2>
        <p class="step-desc">勾选{{ isYesterday ? '昨天' : '今天' }}已完成的任务（自动读取 State 中的完成状态）</p>
        <div v-if="didaMatchedCount > 0" class="dida-sync-note">
          <Smartphone :size="13" />
          <span>已在滴答清单完成 {{ didaMatchedCount }} 项任务，已为你自动勾选；如需调整可直接点击。</span>
        </div>
        <div class="task-review-list">
          <div v-for="task in allTasks" :key="task.id" class="task-review-item"
            :class="{ done: taskCompleted[task.id] }">
            <div class="tri-left">
              <div class="tri-badges">
                <Badge :variant="subjectBadgeVariant(task.subject)" size="sm">{{ subjectLabel(task.subject) }}</Badge>
                <Badge v-if="timeTrackingEnabled && task.estimated_hours > 0" variant="default" size="sm">
                  <Clock :size="12" />
                  ≈{{ formatHours(task.estimated_hours) }}
                </Badge>
                <Badge v-if="timeTrackingEnabled && (taskActualMinutes[task.id] ?? 0) > 0" variant="info" size="sm">
                  <Clock :size="12" />
                  {{ formatMinutes(taskActualMinutes[task.id] ?? 0) }}
                </Badge>
              </div>
              <span class="tri-title">{{ task.title }}</span>
            </div>
            <button type="button" class="check-btn" :class="{ checked: taskCompleted[task.id] }"
              :aria-pressed="taskCompleted[task.id]"
              @click="taskCompleted[task.id] = !taskCompleted[task.id]">
              <CheckCircle2 v-if="taskCompleted[task.id]" :size="18" />
              <Circle v-else :size="18" />
              {{ taskCompleted[task.id] ? '已完成' : '未完成' }}
            </button>
          </div>
        </div>
      </Card>

      <!-- Step 2: Blockers -->
      <Card v-if="step === 1 && incompleteTasks.length > 0" padding="lg" class="step-card">
        <h2 class="step-title">未完成原因</h2>
        <p class="step-desc">以下{{ isYesterday ? '昨天' : '今天' }}的任务未能完成，请选择原因</p>
        <div v-for="task in incompleteTasks" :key="task.id" class="blocker-item">
          <div class="blocker-task">
            <span class="blocker-title">{{ task.title }}</span>
          </div>
          <div class="blocker-chips">
            <button v-for="opt in blockerOptions" :key="opt.value" type="button" class="blocker-chip"
              :class="{ active: (taskBlockers[task.id] ?? []).includes(opt.value) }"
              @click="toggleBlocker(task.id, opt.value)">{{ opt.label }}</button>
          </div>
          <input v-if="(taskBlockers[task.id] ?? []).includes('other')"
            v-model="blockerNotes[task.id]" type="text" class="field-input" placeholder="请说明具体原因..." />
        </div>
      </Card>

      <!-- Step 3: Overall Feeling -->
      <Card v-if="step === 2" padding="lg" class="step-card">
        <h2 class="step-title">整体学习感受</h2>
        <p class="step-desc">{{ isYesterday ? '昨天' : '今天' }}整体学习感觉如何？</p>
        <div class="feeling-grid">
            <button v-for="opt in feelingOptions" :key="opt.value" type="button" class="feeling-chip"
            :aria-pressed="overallFeeling === opt.value"
            :class="{ active: overallFeeling === opt.value }" @click="overallFeeling = opt.value">
            <span class="feeling-emoji">{{ opt.icon }}</span>
            <span>{{ opt.label }}</span>
          </button>
        </div>

        <div class="review-subsection">
          <h3 class="subsection-title">今天的任务安排量是否合理？</h3>
          <p class="step-desc">这个反馈会参与下周计划量校准，不会因为一次反馈大幅改变计划。</p>
          <div class="difficulty-grid">
            <button v-for="opt in workloadOptions" :key="opt.value" type="button" class="difficulty-chip"
              :aria-pressed="workloadFeedback === opt.value"
              :class="{ active: workloadFeedback === opt.value }"
              @click="workloadFeedback = opt.value">{{ opt.label }}</button>
          </div>
        </div>

        <div class="review-subsection">
          <h3 class="subsection-title">今天是否有临时外部影响？</h3>
          <p class="step-desc">外部异常不会被当成你的长期学习能力。</p>
          <div class="difficulty-grid">
            <button v-for="opt in externalOptions" :key="opt.value" type="button" class="difficulty-chip"
              :aria-pressed="externalInterference === opt.value"
              :class="{ active: externalInterference === opt.value }"
              @click="externalInterference = opt.value">{{ opt.label }}</button>
          </div>
        </div>
      </Card>

      <!-- Step 4: Main Difficulty -->
      <Card v-if="step === 3" padding="lg" class="step-card">
        <h2 class="step-title">最大困难（可选）</h2>
        <p class="step-desc">{{ isYesterday ? '昨天' : '今天' }}最大的困难是什么？用于 Analytics 分析。</p>
        <div class="difficulty-grid">
          <button v-for="opt in difficultyOptions" :key="opt.value" type="button" class="difficulty-chip"
            :aria-pressed="mainDifficulty === opt.value"
            :class="{ active: mainDifficulty === opt.value }"
            @click="mainDifficulty = mainDifficulty === opt.value ? '' : opt.value">{{ opt.label }}</button>
        </div>
      </Card>

      <!-- Step 5: 计划外学习 (extra, optional) -->
      <Card v-if="step === 4" padding="lg" class="step-card">
        <h2 class="step-title">计划外学习（可选）</h2>
        <p class="step-desc">如果{{ isYesterday ? '昨天' : '今天' }}学了计划之外的内容（例如提前学到了后面的章节），点选你实际到达的最新章节即可：从目前进度到所选章节之间会整段记为本次计划外（状态推进到「基础」；是否需要「强化中/掌握」由你在进度表内自行推进），AI 会以这份实际进度为基准修正后续计划。只学到某章内的部分知识点时，展开该章逐条勾选即可。若点选了已学区域内的章节，说明系统记录快于实际进度，可确认后将进度回退到该章。</p>
        <div class="overcompletion-toggle">
          <button type="button" class="oc-switch" :class="{ active: hasOvercompletion }"
            @click="hasOvercompletion = !hasOvercompletion">
            <CheckCircle2 v-if="hasOvercompletion" :size="18" />
            <Circle v-else :size="18" />
            {{ hasOvercompletion ? '已开启计划外学习记录' : '我今天有计划外的学习' }}
          </button>
        </div>
        <div v-if="hasOvercompletion" class="overcompletion-list">
          <!-- 科目切换 -->
          <div class="oc-subjects">
            <button
              v-for="s in OC_SUBJECTS"
              :key="s"
              type="button"
              class="oc-subject-chip"
              :class="{ active: activeSubject === s, disabled: !hasTable(s) }"
              :disabled="!hasTable(s)"
              :title="hasTable(s) ? subjectLabel(s) : `${subjectLabel(s)}（该科目还没有进度表）`"
              @click="activeSubject = s"
            >
              <FolderOpen v-if="hasTable(s)" :size="13" />
              <AlertTriangle v-else :size="13" />
              {{ subjectLabel(s) }}
            </button>
          </div>

          <!-- 主体：加载中 / 无表 / 章节清单 -->
          <LoadingSpinner v-if="progressLoading" :size="24" label="加载进度表..." class="oc-loading" />
          <EmptyState
            v-else-if="!activeTable"
            :title="`${subjectLabel(activeSubject)}还没有进度表`"
            description="请先到「进度」页创建或启用一份进度表，再回来记录计划外进度。"
          >
            <template #actions>
              <Button variant="primary" size="sm" @click="router.push({ path: '/progress', query: { subject: activeSubject } })">
                <FolderOpen :size="14" /> 前往进度页创建
              </Button>
            </template>
          </EmptyState>

          <template v-else>
            <!-- 科目内多张进度表切换（专业课通常 = 总进度表 + 各指定教材表） -->
            <div v-if="tablesOfSubject(activeSubject).length > 1" class="oc-tables">
              <span class="oc-tables-label">选择进度表</span>
              <button
                v-for="t in tablesOfSubject(activeSubject)"
                :key="t.id"
                type="button"
                class="oc-table-chip"
                :class="{ active: activeTable?.id === t.id }"
                :title="`${t.name}（${t.variant}）· 已勾选 ${ocCountOfTable(activeSubject, t)} 项`"
                @click="setViewTable(activeSubject, t.id)"
              >
                <FolderOpen :size="12" />
                <span class="oc-table-chip-name">{{ t.name }}</span>
                <span v-if="ocCountOfTable(activeSubject, t) > 0" class="oc-table-chip-count">
                  {{ ocCountOfTable(activeSubject, t) }}
                </span>
              </button>
            </div>

            <div class="oc-table-caption">
              <Badge variant="default">{{ subjectLabel(activeSubject) }}</Badge>
              <Badge variant="info">{{ activeTable.variant }}</Badge>
              <span class="oc-table-name">{{ activeTable.name }}（{{ activeTable.nodes.length }} 节点）</span>
            </div>

            <p v-if="!chapterNodes.length && !knowledgeNodes.length" class="oc-empty-hint">
              该进度表还没有可点选的{{ hasChapters ? '章节' : '节点' }}，可在下方新建一个。
            </p>

            <!-- 进度指针：点击你实际到达的最新章节，其前未记录内容自动整段补记 -->
            <div v-if="ocUnits.length" class="oc-progress-bar">
              <div class="oc-progress-item">
                <span class="oc-progress-label">目前进度</span>
                <span v-if="ocTailUnit" class="oc-progress-name">{{ ocTailUnit.title }}</span>
                <span v-else class="oc-progress-name">尚未开始</span>
              </div>
              <span class="oc-progress-legend">
                <span class="oc-legend-dot oc-legend-now"></span>本次
                <span class="oc-legend-dot oc-legend-learned"></span>已学
                <span class="oc-legend-dot oc-legend-wait"></span>待学
              </span>
            </div>

            <div v-if="ocUnits.length" class="oc-section-desc">
              <Info :size="13" />
              <span>在下方点选你本次<strong>实际到达的最新{{ hasChapters ? '章节' : '内容' }}</strong>即可：从「目前进度」到所选章节之间未记录的内容会自动整段补记为本次计划外（状态推进到「基础」）。只学到某章中间的<strong>部分知识点</strong>时，展开该章逐条勾选即可，不会把整章记完。点选「已学」区域内的章节，说明系统记录的进度<strong>快于</strong>实际进度，可确认后回退。</span>
            </div>

            <div v-if="ocUnits.length" ref="ocListRef" class="oc-node-list oc-pointer-list">
              <div v-for="(u, ui) in ocUnits" :key="u.id" class="oc-unit-block">
                <div
                  class="oc-node-row oc-pointer-row"
                  :class="{
                    checked: isChecked(activeSubject, u),
                    learned: !isChecked(activeSubject, u) && ocIsLearned(u),
                    cur: ui === ocTailIdx || (ocTailIdx < 0 && ui === 0),
                  }"
                  :data-uid="u.id"
                >
                  <button
                    v-if="u.level === 'chapter' && childrenOfChapter(u.id).length"
                    type="button"
                    class="oc-expand"
                    :class="{ open: isOcExpanded(u.id) }"
                    :title="isOcExpanded(u.id) ? '收起本章知识点' : '展开本章知识点，可逐条勾选本次学到的具体知识点'"
                    @click="toggleOcExpand(u.id)"
                  >
                    <ChevronRight :size="14" />
                  </button>
                  <span v-else class="oc-expand-spacer"></span>
                  <button type="button" class="oc-pointer-main" :title="pointerRowHint(ui)" @click="onUnitClick(u)">
                    <span class="oc-check" :class="{ active: isChecked(activeSubject, u) }">
                      <CheckCircle2 v-if="isChecked(activeSubject, u)" :size="17" />
                      <CheckCircle2 v-else-if="ocIsLearned(u)" :size="17" />
                      <Circle v-else :size="17" />
                    </span>
                    <span class="oc-node-icon">
                      <FolderOpen v-if="u.level === 'chapter'" :size="13" />
                      <CircleDot v-else :size="13" />
                    </span>
                    <span class="oc-node-title">{{ u.title }}</span>
                    <span v-if="isChecked(activeSubject, u)" class="oc-kid-count oc-tag-now">本次</span>
                    <span v-else-if="ocIsLearned(u)" class="oc-kid-count oc-tag-learned">已学</span>
                    <span
                      v-if="u.level === 'chapter' && childrenOfChapter(u.id).length"
                      class="oc-kid-count"
                      :class="{
                        'oc-partial':
                          !isChecked(activeSubject, u) &&
                          !ocIsLearned(u) &&
                          chapterKidStats(u.id).learned > 0,
                      }"
                    >
                      {{ chapterKidStats(u.id).learned }}/{{ chapterKidStats(u.id).total }} 知识点
                    </span>
                  </button>
                </div>

                <!-- 展开章节：逐个勾选本章内本次学到的具体知识点（不改变章节顺序进度指针） -->
                <div
                  v-if="u.level === 'chapter' && isOcExpanded(u.id) && childrenOfChapter(u.id).length"
                  class="oc-node-children"
                >
                  <div
                    v-for="k in childrenOfChapter(u.id)"
                    :key="k.id"
                    class="oc-node-row oc-knowledge-row"
                    :class="{ checked: isChecked(activeSubject, k) }"
                  >
                    <button
                      type="button"
                      class="oc-check"
                      :class="{ active: isChecked(activeSubject, k) }"
                      :title="
                        isChecked(activeSubject, k)
                          ? '撤销本次对该知识点的记录'
                          : '记为本次学到的知识点（状态推进到「基础」）'
                      "
                      @click="toggleKnowledgePoint(activeSubject, k)"
                    >
                      <CheckCircle2 v-if="isChecked(activeSubject, k)" :size="16" />
                      <CheckCircle2 v-else-if="ocIsLearned(k)" :size="16" />
                      <Circle v-else :size="16" />
                    </button>
                    <span class="oc-node-icon"><CircleDot :size="13" /></span>
                    <span class="oc-node-title">{{ k.title }}</span>
                    <span v-if="isChecked(activeSubject, k)" class="oc-kid-count oc-tag-now">本次</span>
                    <span v-else-if="ocIsLearned(k)" class="oc-kid-count oc-tag-learned">已学</span>
                  </div>
                </div>
              </div>
            </div>

            <!-- 未挂靠章节的知识点：保留逐个勾选兜底（不参与章节顺序进度） -->
            <template v-if="hasChapters && orphanKnowledge.length">
              <div class="oc-group-title">未分组知识点（单独勾选，不影响章节顺序进度）</div>
              <div class="oc-node-list">
                <div
                  v-for="k in orphanKnowledge"
                  :key="k.id"
                  class="oc-node-row oc-knowledge-row"
                  :class="{ checked: isChecked(activeSubject, k) }"
                >
                  <button
                    type="button"
                    class="oc-check"
                    :class="{ active: isChecked(activeSubject, k) }"
                    @click="toggleKnowledgePoint(activeSubject, k)"
                  >
                    <CheckCircle2 v-if="isChecked(activeSubject, k)" :size="16" />
                    <Circle v-else :size="16" />
                  </button>
                  <span class="oc-node-icon"><CircleDot :size="13" /></span>
                  <span class="oc-node-title">{{ k.title }}</span>
                </div>
              </div>
            </template>

            <!-- 点选已学区域：系统记录进度快于实际进度 → 确认回退 -->
            <div v-if="correctionTarget" class="oc-correction">
              <AlertTriangle :size="17" class="oc-correction-icon" />
              <div class="oc-correction-body">
                <p class="oc-correction-title">你点选了已学区域内的章节：系统记录进度快于实际进度</p>
                <p class="oc-correction-text">
                  系统记录的进度为已学至「{{ ocTailUnit?.title ?? '—' }}」，而你选择的「{{ correctionTarget.title }}」位于其内，
                  说明目前的记录<strong>快于</strong>你的实际学习进度（此前记录超前）。
                  确认后会把{{ subjectLabel(activeSubject) }}的进度基准回退到该章：其后的章节/知识点恢复为未学、本轮相应记录一并撤销，后续计划以复盘重排结果为准。
                </p>
                <div class="oc-correction-actions">
                  <Button variant="danger" size="sm" :loading="savingProgress" @click="confirmCorrection">
                    确认回退进度
                  </Button>
                  <Button variant="ghost" size="sm" @click="cancelCorrection">取消</Button>
                </div>
              </div>
            </div>

            <p v-if="ocHint" class="oc-hint"><Info :size="13" /> {{ ocHint }}</p>

            <!-- 新建节点 -->
            <div class="oc-create">
              <input
                v-model="newChapterTitle"
                type="text"
                class="field-input oc-create-input"
                :placeholder="hasChapters ? '新建章节，如：第三章 微分中值定理' : '新建节点，如：函数的概念及表示法'"
                @keydown.enter="createChapter(activeSubject)"
              />
              <Button variant="secondary" size="sm" @click="createChapter(activeSubject)">
                <Plus :size="14" /> 新建{{ hasChapters ? '章节' : '节点' }}
              </Button>
            </div>
            <p v-if="newChapterError" class="oc-error">{{ newChapterError }}</p>

            <!-- 历史未匹配条目兜底：可编辑，重新提交不丢数据 -->
            <div v-if="unmatchedEntries.length" class="oc-unmatched">
              <div class="oc-unmatched-title">以下记录未能匹配到进度表节点（可在下方修改后保留）</div>
              <div v-for="oc in unmatchedEntries" :key="oc.chapter_reached + oc.subject" class="oc-item">
                <div class="oc-row">
                  <span class="oc-unmatched-subject">{{ subjectLabel(oc.subject) }}</span>
                  <input v-model="oc.chapter_reached" type="text" class="field-input oc-input"
                    placeholder="实际学到的内容/章节" />
                  <Button variant="ghost" size="sm" @click="overcompletions = overcompletions.filter(x => x !== oc)">
                    <AlertTriangle :size="14" />
                  </Button>
                </div>
                <input v-model="oc.note" type="text" class="field-input" placeholder="备注（可选）" />
              </div>
            </div>
          </template>
        </div>
      </Card>

      <!-- Navigation -->
      <div class="step-nav">
        <Button v-if="step > 0" variant="ghost" @click="goPrev"><ArrowLeft :size="16" /> 上一步</Button>
        <div class="nav-right">
          <Button
            v-if="step < totalSteps - 1"
            variant="primary"
            :disabled="!canNext()"
            @click="goNext"
          >
            下一步 <ArrowRight :size="16" />
          </Button>
          <Button v-else variant="primary" @click="doSubmit" :loading="submitting">
            <Sparkles :size="16" /> 提交复盘
          </Button>
        </div>
      </div>
    </template>
    </div>
    </transition>
  </div>
</template>

<style scoped>
.review-view {
  max-width: 720px;
  margin: 0 auto;
  padding: var(--space-8);
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

/* Date bar */
.date-bar {
  position: sticky;
  top: 0;
  z-index: 10;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  background: var(--bg-primary);
  padding: var(--space-4) 0;
  margin: 0 calc(-1 * var(--space-8));
  padding-left: var(--space-8);
  padding-right: var(--space-8);
  flex-wrap: wrap;
}

.date-nav {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
}

.date-display {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  min-width: 160px;
  justify-content: center;
}

.date-tag {
  font-size: var(--text-xs);
  padding: 1px 8px;
  border-radius: var(--radius-full);
  font-weight: var(--font-medium);
}

.today-tag { background: var(--accent-subtle); color: var(--accent); }
.yesterday-tag { background: var(--color-warning-subtle); color: var(--color-warning); }
.past-tag { background: var(--bg-tertiary); color: var(--text-tertiary); }
.future-tag { background: var(--bg-overlay); color: var(--text-quaternary); }

.date-actions {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  position: relative;
}

.history-dropdown-wrapper {
  position: relative;
}

.history-dropdown {
  position: absolute;
  top: calc(100% + 4px);
  right: 0;
  min-width: 200px;
  max-height: 320px;
  overflow-y: auto;
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg);
  z-index: 20;
  padding: var(--space-1);
}

.dropdown-header {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  padding: var(--space-2) var(--space-3);
  font-weight: var(--font-medium);
  border-bottom: 1px solid var(--divider-color);
  margin-bottom: var(--space-1);
}

.dropdown-empty {
  font-size: var(--text-sm);
  color: var(--text-tertiary);
  padding: var(--space-3);
  text-align: center;
}

.dropdown-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  padding: var(--space-2) var(--space-3);
  background: transparent;
  border: none;
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
  color: var(--text-primary);
  cursor: pointer;
  font-family: inherit;
  transition: background var(--transition-fast);
}

.dropdown-item:hover { background: var(--bg-overlay); }
.dropdown-item.active { background: var(--accent-subtle); color: var(--accent); font-weight: var(--font-semibold); }

.item-tag {
  font-size: 10px;
  color: var(--text-tertiary);
  background: var(--bg-tertiary);
  padding: 1px 6px;
  border-radius: var(--radius-full);
}

/* Backfill banner */
.backfill-banner {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--color-warning-subtle);
  color: var(--color-warning);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
}

.loading-msg {
  text-align: center;
  color: var(--text-tertiary);
  padding: var(--space-8);
}

/* Gate */
.gate-card { text-align: center; }
.gate-hero {
  display: flex; flex-direction: column; align-items: center;
  gap: var(--space-4); padding: var(--space-8) var(--space-4);
}
.gate-icon { color: var(--text-tertiary); }
.gate-title { font-size: var(--text-xl); font-weight: var(--font-bold); color: var(--text-primary); }
.gate-desc { font-size: var(--text-base); color: var(--text-secondary); margin: 0; }
.gate-hint { font-size: var(--text-sm); color: var(--text-tertiary); margin: 0; }
.gate-cancel-btn { margin-top: var(--space-2); }

/* 提交复盘分步进度 */
.submit-steps {
  display: flex; flex-direction: column; align-items: flex-start;
  gap: var(--space-3); margin-top: var(--space-2);
  width: 100%; max-width: 320px;
}
.submit-step {
  display: flex; align-items: center; gap: var(--space-2);
  font-size: var(--text-base); color: var(--text-tertiary);
  transition: color var(--transition-fast);
}
.submit-step.done { color: var(--text-secondary); }
.submit-step.active { color: var(--text-primary); font-weight: var(--font-medium); }
.submit-step-icon {
  display: inline-flex; align-items: center; justify-content: center;
  width: 20px; height: 20px; flex-shrink: 0;
}
.submit-step.done .submit-step-icon,
.submit-step.active .submit-step-icon { color: var(--accent); }
.submit-step-dot {
  width: 8px; height: 8px; border-radius: var(--radius-full);
  background: var(--bg-tertiary);
}

.step-bar {
  display: flex; align-items: center; gap: var(--space-3); padding-bottom: var(--space-2);
}
.step-dots { display: flex; gap: var(--space-2); flex: 1; }
.step-dot {
  width: 100%; height: 4px; background: var(--bg-tertiary);
  border-radius: var(--radius-full); transition: background var(--transition-fast);
}
.step-dot.active { background: var(--accent); }
.step-dot.done { background: var(--color-success); }
.step-label {
  font-size: var(--text-xs); color: var(--text-tertiary);
  font-weight: var(--font-medium); flex-shrink: 0;
}

.step-card { display: flex; flex-direction: column; gap: var(--space-5); }
.review-subsection {
  display: flex; flex-direction: column; gap: var(--space-3);
  padding-top: var(--space-3); border-top: 1px solid var(--border-color);
}
.subsection-title {
  font-size: var(--text-base); font-weight: var(--font-semibold);
  color: var(--text-primary);
}
.step-title {
  font-size: var(--text-xl); font-weight: var(--font-bold);
  color: var(--text-primary); letter-spacing: -0.01em;
}
.step-desc { font-size: var(--text-sm); color: var(--text-secondary); margin: 0; margin-top: -12px; }

/* 滴答回读提示 */
.dida-sync-note {
  display: flex; align-items: center; gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--accent-subtle, color-mix(in srgb, var(--accent) 10%, transparent));
  border-radius: var(--radius-md);
  font-size: var(--text-xs); color: var(--accent, var(--text-secondary));
  margin-top: var(--space-3);
  line-height: var(--leading-normal);
}
.dida-sync-note svg { flex-shrink: 0; }

/* Task list */
.task-review-list { display: flex; flex-direction: column; gap: var(--space-3); }
.task-review-item {
  display: flex; align-items: center; justify-content: space-between; gap: var(--space-3);
  padding: var(--space-3) var(--space-4); background: var(--bg-tertiary);
  border-radius: var(--radius-md); flex-wrap: wrap;
}
.task-review-item.done { opacity: 0.65; background: var(--bg-overlay); }
.tri-left { display: flex; flex-direction: column; gap: var(--space-1); min-width: 0; flex: 1; }
.tri-badges { display: flex; gap: var(--space-1); }
.tri-title {
  font-size: var(--text-sm); font-weight: var(--font-medium);
  color: var(--text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}

.check-btn {
  display: flex; align-items: center; gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border: 1.5px solid var(--border-color); border-radius: var(--radius-md);
  background: var(--bg-elevated); color: var(--text-secondary);
  font-size: var(--text-sm); font-weight: var(--font-medium);
  cursor: pointer; transition: all var(--transition-fast);
  font-family: inherit; white-space: nowrap; flex-shrink: 0;
}
.check-btn:hover { border-color: var(--accent); color: var(--accent); }
.check-btn.checked { border-color: var(--color-success); background: var(--color-success-subtle); color: var(--color-success); font-weight: var(--font-semibold); }

/* Blockers */
.blocker-item {
  display: flex; flex-direction: column; gap: var(--space-3);
  padding: var(--space-4); background: var(--bg-tertiary); border-radius: var(--radius-md);
}
.blocker-task { display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap; }
.blocker-title { font-size: var(--text-base); font-weight: var(--font-semibold); color: var(--text-primary); }
.blocker-chips { display: flex; flex-wrap: wrap; gap: var(--space-2); }
.blocker-chip {
  padding: var(--space-1) var(--space-3); border: 1.5px solid var(--border-color);
  border-radius: var(--radius-full); background: var(--bg-elevated);
  color: var(--text-secondary); font-size: var(--text-xs); font-weight: var(--font-medium);
  cursor: pointer; transition: all var(--transition-fast); font-family: inherit;
}
.blocker-chip:hover { border-color: var(--accent); }
.blocker-chip.active { border-color: var(--color-danger); background: var(--color-danger-subtle); color: var(--color-danger); font-weight: var(--font-semibold); }

.field-input {
  background: var(--bg-elevated); border: 1px solid var(--border-color);
  border-radius: var(--radius-md); padding: var(--space-2) var(--space-3);
  font-size: var(--text-sm); font-family: inherit; color: var(--text-primary);
  width: 100%; outline: none;
}
.field-input:focus { border-color: var(--accent); }

/* Empty step */
.empty-step {
  display: flex; flex-direction: column; align-items: center; gap: var(--space-3);
  padding: var(--space-8) var(--space-4); color: var(--text-secondary);
  font-size: var(--text-sm); text-align: center;
}
.empty-icon { color: var(--color-success); }
.empty-icon.warn { color: var(--color-warning); }

/* Feeling */
.feeling-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: var(--space-3); }
.feeling-chip {
  display: flex; flex-direction: column; align-items: center; gap: var(--space-2);
  padding: var(--space-5) var(--space-3); border: 1.5px solid var(--border-color);
  border-radius: var(--radius-lg); background: var(--bg-elevated);
  cursor: pointer; transition: all var(--transition-fast);
  font-family: inherit; font-size: var(--text-sm); color: var(--text-secondary);
}
.feeling-chip:hover { border-color: var(--accent); color: var(--accent); }
.feeling-chip.active { border-color: var(--accent); background: var(--accent-subtle); color: var(--accent); font-weight: var(--font-semibold); }
.feeling-emoji { font-size: 24px; }

/* Difficulty */
.difficulty-grid { display: flex; flex-wrap: wrap; gap: var(--space-2); }
.difficulty-chip {
  padding: var(--space-2) var(--space-4); border: 1.5px solid var(--border-color);
  border-radius: var(--radius-full); background: var(--bg-elevated);
  color: var(--text-secondary); font-size: var(--text-sm); font-weight: var(--font-medium);
  cursor: pointer; transition: all var(--transition-fast); font-family: inherit;
}
.difficulty-chip:hover { border-color: var(--accent); }
.difficulty-chip.active { border-color: var(--accent); background: var(--accent-subtle); color: var(--accent); }

/* Nav */
.step-nav {
  display: flex; align-items: center; justify-content: space-between;
  gap: var(--space-3); padding-top: var(--space-4);
}
.nav-right { margin-left: auto; }

/* Done / existing review */
.done-card { text-align: center; }
.done-hero {
  display: flex; flex-direction: column; align-items: center; gap: var(--space-4);
  padding: var(--space-8) var(--space-4);
}
.done-badge {
  width: 72px; height: 72px; display: flex; align-items: center; justify-content: center;
  background: var(--color-success-subtle); color: var(--color-success); border-radius: var(--radius-lg);
}
.done-title { font-size: var(--text-2xl); font-weight: var(--font-bold); color: var(--text-primary); letter-spacing: -0.02em; }
.done-desc { font-size: var(--text-base); color: var(--text-secondary); margin: 0; }
.done-hint { font-size: var(--text-sm); color: var(--text-tertiary); margin: 0; }

.regen-banner {
  display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap;
  padding: var(--space-3) var(--space-4);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  margin-top: var(--space-3);
  background: var(--color-info-subtle, var(--bg-tertiary));
  color: var(--color-info, var(--text-secondary));
}

/* ── 重排变更明细：悬停查看 ── */
.regen-details-trigger {
  display: inline-flex; align-items: center; gap: var(--space-1);
  font-size: var(--text-xs); cursor: help;
  color: inherit; opacity: 0.9;
  position: relative;
}
.regen-details-trigger:hover {
  text-decoration: underline; opacity: 1;
}
.regen-details-popover {
  display: none;
  position: absolute;
  top: 100%;
  left: 0;
  z-index: 40;
  min-width: 320px;
  max-width: 460px;
  padding: var(--space-3);
  background: var(--bg-elevated, var(--bg-primary));
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg, 0 12px 32px rgba(0, 0, 0, 0.18));
  text-align: left;
  color: var(--text-primary);
  /* 弹窗本体保持 overflow: visible，使过渡桥（::before）不被裁剪，
     内部滚动交给 .regen-details-inner；margin-top 制造视觉间隙但鼠标可经桥无间断滑入 */
  overflow: visible;
  margin-top: 8px;
}
/* 过渡桥：覆盖在 trigger 与弹窗之间的空白区域，消除悬停死区 */
.regen-details-popover::before {
  content: "";
  position: absolute;
  top: -8px;
  left: -1px;
  right: -1px;
  height: 8px;
}
/* 内部滚动容器：真正承载滚动的区域 */
.regen-details-inner {
  max-height: 320px;
  overflow-y: auto;
}
.regen-details-trigger:hover .regen-details-popover,
.regen-details-trigger:focus-within .regen-details-popover {
  display: block;
}
.regen-detail-fallback {
  font-size: var(--text-xs); color: var(--text-tertiary);
  margin-bottom: var(--space-2); line-height: var(--leading-normal);
}
.regen-detail-day {
  padding: var(--space-2) 0;
  border-top: 1px solid var(--border-color);
  display: flex; flex-direction: column; gap: var(--space-1);
}
.regen-detail-day:first-child { border-top: none; padding-top: 0; }
.regen-detail-date {
  font-size: var(--text-xs); font-weight: var(--font-medium);
  color: var(--text-secondary);
}
.regen-detail-item {
  font-size: var(--text-xs); line-height: var(--leading-normal);
  margin-left: var(--space-2); word-break: break-all;
}
.regen-detail-item.add { color: var(--color-success); }
.regen-detail-item.remove { color: var(--color-danger); }
.regen-detail-item.modify { color: var(--color-warning); }
.regen-detail-item.none { color: var(--text-tertiary); }
.regen-banner.regen-loading {
  background: var(--color-warning-subtle, var(--bg-tertiary));
  color: var(--color-warning, var(--text-primary));
}
.regen-banner.regen-error {
  background: var(--color-danger-subtle, var(--bg-tertiary));
  color: var(--color-danger, var(--text-primary));
}
.regen-retry-btn {
  margin-left: auto;
  flex-shrink: 0;
}

.review-summary {
  display: flex; flex-direction: column; gap: var(--space-2);
  padding: var(--space-4); background: var(--bg-tertiary);
  border-radius: var(--radius-md); width: 100%; max-width: 320px;
}
.summary-row {
  display: flex; justify-content: space-between; align-items: center;
  font-size: var(--text-sm);
}
.summary-label { color: var(--text-tertiary); }
.summary-value { color: var(--text-primary); font-weight: var(--font-semibold); }

/* Task review details (read-only) */
.task-reviews-list {
  width: 100%;
  max-width: 560px;
  display: flex; flex-direction: column; gap: var(--space-3);
  text-align: left;
}
.task-reviews-title {
  font-size: var(--text-sm); font-weight: var(--font-semibold); color: var(--text-secondary);
  text-transform: uppercase; letter-spacing: 0.04em;
}
.task-review-row {
  display: flex; align-items: center; gap: var(--space-3);
  padding: var(--space-3); background: var(--bg-tertiary);
  border-radius: var(--radius-md); flex-wrap: wrap;
}
.task-review-row.incomplete { opacity: 0.7; }
.task-review-row.abandoned { opacity: 0.5; }
.trr-left {
  display: flex; align-items: center; gap: var(--space-2);
  flex-shrink: 0; min-width: 120px;
}
.trr-status-icon { color: var(--text-tertiary); }
.task-review-row.completed .trr-status-icon { color: var(--color-success); }
.task-review-row.overcompletion .trr-status-icon { color: var(--accent); }
.trr-title-wrap { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.trr-title {
  font-size: var(--text-sm); font-weight: var(--font-semibold);
  color: var(--text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  max-width: 320px;
}
.trr-meta { display: flex; align-items: center; gap: var(--space-2); }
.trr-subject {
  font-size: var(--text-xs); color: var(--text-tertiary);
  background: var(--bg-overlay); padding: 1px 6px; border-radius: var(--radius-full);
}
.trr-status { font-size: var(--text-sm); font-weight: var(--font-medium); color: var(--text-secondary); }
.trr-right {
  display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap;
  flex: 1;
}
.trr-chip {
  font-size: var(--text-xs);
  padding: 2px 8px;
  border-radius: var(--radius-full);
  font-weight: var(--font-medium);
}
.trr-chip.mastery { background: var(--accent-subtle); color: var(--accent); }
.trr-chip.blocker { background: var(--color-danger-subtle); color: var(--color-danger); }
.trr-chip.estimate { background: var(--bg-overlay); color: var(--text-tertiary); }
.trr-chip.actual { background: var(--color-info-subtle, var(--bg-tertiary)); color: var(--color-info, var(--text-secondary)); }
.trr-chip.estimate svg, .trr-chip.actual svg { vertical-align: -1px; margin-right: 2px; }
.trr-note { font-size: var(--text-xs); color: var(--text-tertiary); font-style: italic; }

/* Overcompletion */
.overcompletion-toggle { margin-bottom: var(--space-3); }
.oc-switch {
  display: flex; align-items: center; gap: var(--space-2);
  padding: var(--space-3) var(--space-4); border: 1.5px solid var(--border-color);
  border-radius: var(--radius-md); background: var(--bg-elevated);
  color: var(--text-secondary); font-size: var(--text-sm); font-weight: var(--font-medium);
  cursor: pointer; transition: all var(--transition-fast); font-family: inherit; width: 100%;
}
.oc-switch:hover { border-color: var(--accent); }
.oc-switch.active { border-color: var(--accent); background: var(--accent-subtle); color: var(--accent); font-weight: var(--font-semibold); }
.overcompletion-list { display: flex; flex-direction: column; gap: var(--space-3); }
.oc-item {
  display: flex; flex-direction: column; gap: var(--space-2);
  padding: var(--space-3); background: var(--bg-tertiary); border-radius: var(--radius-md);
}
.oc-row { display: flex; align-items: center; gap: var(--space-2); }
.oc-select {
  background: var(--bg-elevated); border: 1px solid var(--border-color);
  border-radius: var(--radius-md); padding: var(--space-2) var(--space-3);
  font-size: var(--text-sm); font-family: inherit; color: var(--text-primary);
  outline: none; min-width: 110px;
}
.oc-input { flex: 1; }

/* 计划外学习：科目 chip + 章节清单 */
.oc-subjects { display: flex; gap: var(--space-2); flex-wrap: wrap; }
.oc-subject-chip {
  display: inline-flex; align-items: center; gap: var(--space-1);
  padding: 5px 12px; border: 1px solid var(--border-color); border-radius: var(--radius-full);
  background: var(--bg-primary); color: var(--text-secondary);
  font-family: inherit; font-size: var(--text-xs); font-weight: var(--font-medium);
  cursor: pointer; transition: all var(--transition-fast);
}
.oc-subject-chip:hover:not(:disabled) { border-color: var(--border-color-strong); color: var(--text-primary); }
.oc-subject-chip.active { border-color: var(--accent); color: var(--accent); background: var(--accent-subtle); }
.oc-subject-chip.disabled { opacity: 0.45; cursor: not-allowed; }

/* 科目内多张进度表切换（专业课多教材 / 多考纲方案） */
.oc-tables {
  display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap;
  padding: var(--space-2) var(--space-3);
  background: var(--bg-tertiary); border-radius: var(--radius-md);
}
.oc-tables-label { font-size: var(--text-xs); color: var(--text-tertiary); flex-shrink: 0; }
.oc-table-chip {
  display: inline-flex; align-items: center; gap: 5px; max-width: 220px;
  padding: 3px 10px; border: 1px solid var(--border-color); border-radius: var(--radius-full);
  background: var(--bg-primary); color: var(--text-secondary);
  font-family: inherit; font-size: var(--text-xs); font-weight: var(--font-medium);
  cursor: pointer; transition: all var(--transition-fast);
}
.oc-table-chip:hover { border-color: var(--border-color-strong); color: var(--text-primary); }
.oc-table-chip.active { border-color: var(--accent); color: var(--accent); background: var(--accent-subtle); }
.oc-table-chip-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.oc-table-chip-count {
  display: inline-flex; align-items: center; justify-content: center; min-width: 16px; height: 16px;
  padding: 0 4px; border-radius: var(--radius-full);
  background: var(--accent); color: #fff; font-size: 10px; font-weight: var(--font-semibold);
}
.oc-table-chip:not(.active) .oc-table-chip-count { background: var(--text-quaternary); }

.oc-loading { margin: var(--space-4) auto; }
.oc-table-caption {
  display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap;
  font-size: var(--text-xs); color: var(--text-tertiary);
}
.oc-table-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.oc-empty-hint { margin: 0; font-size: var(--text-xs); color: var(--text-tertiary); }

/* 章节 → 知识点 两级选择树 */
.oc-unit-block { display: flex; flex-direction: column; gap: 2px; }
.oc-expand {
  display: inline-flex; align-items: center; justify-content: center;
  width: 18px; height: 18px; flex-shrink: 0; padding: 0;
  border: none; background: transparent; color: var(--text-tertiary);
  cursor: pointer; border-radius: var(--radius-sm);
  transition: transform var(--transition-fast), background var(--transition-fast);
}
.oc-expand:hover { color: var(--text-primary); background: var(--bg-tertiary); }
.oc-expand.open { transform: rotate(90deg); }
.oc-expand-spacer { width: 18px; flex-shrink: 0; }
.oc-node-children {
  display: flex; flex-direction: column; gap: 2px;
  margin: 0 0 2px 27px; padding-left: 11px;
  border-left: 1.5px dashed var(--border-color);
}
.oc-knowledge-row { padding-top: 5px; padding-bottom: 5px; background: var(--bg-tertiary); }
.oc-knowledge-row .oc-node-title { font-weight: var(--font-normal); }
.oc-group-title {
  margin: var(--space-2) var(--space-3) 0;
  font-size: var(--text-xs); color: var(--text-tertiary);
}
.oc-kid-count, .oc-partial {
  flex-shrink: 0; font-size: var(--text-xs);
  padding: 1px 8px; border-radius: var(--radius-full);
}
.oc-kid-count { color: var(--text-tertiary); background: var(--bg-overlay); }
.oc-partial { color: var(--accent); background: var(--accent-subtle); }
.oc-knowledge-row .oc-node-title { font-weight: var(--font-normal); }
.oc-node-list { display: flex; flex-direction: column; gap: var(--space-1); }
.oc-node-row {
  display: flex; align-items: center; gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-color); border-radius: var(--radius-md);
  background: var(--bg-elevated); transition: border-color var(--transition-fast), opacity var(--transition-fast);
}
.oc-node-row:hover { border-color: var(--border-color-strong); }
.oc-node-row.drag-over { border-color: var(--accent); background: var(--accent-subtle); }
.oc-node-row.dragging { opacity: 0.4; }
.oc-node-row.checked { border-color: var(--accent); background: var(--accent-subtle); }
.oc-check {
  display: inline-flex; align-items: center; justify-content: center;
  padding: 0; border: none; background: transparent; color: var(--text-quaternary);
  cursor: pointer; flex-shrink: 0;
}
.oc-check.active { color: var(--color-success, #16a34a); }
.oc-node-icon { color: var(--text-quaternary); display: inline-flex; flex-shrink: 0; }
.oc-node-title { flex: 1; min-width: 0; font-size: var(--text-sm); font-weight: var(--font-medium); color: var(--text-primary); word-break: break-word; }
.oc-create { display: flex; align-items: center; gap: var(--space-2); }
.oc-create-input { flex: 1; }
.oc-error { margin: 0; font-size: var(--text-xs); color: var(--color-danger); }
.oc-unmatched { display: flex; flex-direction: column; gap: var(--space-2); }
.oc-unmatched-title { font-size: var(--text-xs); color: var(--text-tertiary); }
.oc-unmatched-subject { flex-shrink: 0; font-size: var(--text-xs); color: var(--text-tertiary); background: var(--bg-tertiary); padding: 2px 8px; border-radius: var(--radius-full); }

/* ── 计划外学习 v2：进度指针 ── */
.oc-progress-bar {
  display: flex; align-items: center; justify-content: space-between; gap: var(--space-2);
  padding: var(--space-2) var(--space-3); background: var(--bg-tertiary);
  border-radius: var(--radius-md); flex-wrap: wrap;
}
.oc-progress-item { display: inline-flex; align-items: center; gap: var(--space-2); min-width: 0; }
.oc-progress-label { font-size: var(--text-xs); color: var(--text-tertiary); flex-shrink: 0; }
.oc-progress-name {
  font-size: var(--text-sm); font-weight: var(--font-semibold); color: var(--accent);
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 260px;
}
.oc-progress-legend { display: inline-flex; align-items: center; gap: 5px; font-size: var(--text-xs); color: var(--text-tertiary); }
.oc-legend-dot { width: 8px; height: 8px; border-radius: var(--radius-full); display: inline-block; }
.oc-legend-now { background: var(--accent); }
.oc-legend-learned { background: var(--text-quaternary); }
.oc-legend-wait { border: 1.5px solid var(--border-color-strong); background: transparent; }
.oc-section-desc {
  display: flex; align-items: flex-start; gap: var(--space-2);
  padding: var(--space-2) var(--space-3); border-radius: var(--radius-md);
  background: var(--accent-subtle, color-mix(in srgb, var(--accent) 8%, transparent));
  color: var(--text-secondary); font-size: var(--text-xs); line-height: var(--leading-normal);
}
.oc-section-desc svg { flex-shrink: 0; margin-top: 1px; color: var(--accent); }
.oc-pointer-list { gap: var(--space-1); }
.oc-pointer-row {
  padding: 2px 4px; gap: 2px;
}
.oc-pointer-main {
  display: flex; align-items: center; gap: var(--space-2);
  flex: 1; min-width: 0;
  padding: 4px 6px; border-radius: var(--radius-sm);
  border: none; background: transparent; color: inherit;
  font-family: inherit; text-align: left; cursor: pointer;
  transition: background var(--transition-fast);
}
.oc-pointer-main:hover { background: var(--bg-tertiary); }
.oc-pointer-row.learned { opacity: 0.72; }
.oc-pointer-row.learned .oc-check { color: var(--text-quaternary); }
.oc-pointer-row.cur {
  border-color: var(--accent); background: var(--bg-tertiary);
  box-shadow: inset 3px 0 0 var(--accent);
}
.oc-tag-now { color: var(--accent); background: var(--accent-subtle); font-weight: var(--font-semibold); }
.oc-tag-learned { color: var(--text-tertiary); background: var(--bg-overlay); }
.oc-correction {
  display: flex; gap: var(--space-2); padding: var(--space-3) var(--space-4);
  border: 1px solid var(--color-danger, #dc2626); border-radius: var(--radius-md);
  background: var(--color-danger-subtle, color-mix(in srgb, var(--color-danger) 8%, transparent));
}
.oc-correction-icon { color: var(--color-danger); flex-shrink: 0; margin-top: 1px; }
.oc-correction-body { display: flex; flex-direction: column; gap: var(--space-1); min-width: 0; }
.oc-correction-title { margin: 0; font-size: var(--text-sm); font-weight: var(--font-semibold); color: var(--text-primary); }
.oc-correction-text { margin: 0; font-size: var(--text-xs); color: var(--text-secondary); line-height: var(--leading-normal); }
.oc-correction-actions { display: flex; gap: var(--space-2); margin-top: var(--space-2); }
.oc-hint { display: flex; align-items: flex-start; gap: var(--space-2); margin: 0; font-size: var(--text-xs); color: var(--text-secondary); line-height: var(--leading-normal); }
.oc-hint svg { flex-shrink: 0; margin-top: 1px; color: var(--accent); }

/* ── 日期切换过渡动画 ── */
.date-slide-left-enter-active,
.date-slide-left-leave-active,
.date-slide-right-enter-active,
.date-slide-right-leave-active {
  transition: transform 0.22s ease, opacity 0.22s ease;
}

.date-slide-left-enter-from {
  transform: translateX(12px);
  opacity: 0;
}
.date-slide-left-leave-to {
  transform: translateX(-12px);
  opacity: 0;
}

.date-slide-right-enter-from {
  transform: translateX(-12px);
  opacity: 0;
}
.date-slide-right-leave-to {
  transform: translateX(12px);
  opacity: 0;
}
</style>
