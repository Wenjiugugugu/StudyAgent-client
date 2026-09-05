<script setup lang="ts">
/**
 * 各科「进度表」编辑器（考纲方案变体驱动）
 *
 * 特性（相对旧版）：
 * 1. 按「考纲方案」(variant) 定位与生成（数一/数二/数三/英一/英二/408/307/政治…）
 * 2. 两级节点结构：章节(chapter) / 知识点(knowledge)，知识点通过 parent_id 归属章节，
 *    界面按「章节→知识点」渲染树；无章节数据的旧表回退为按 phase 分组。
 * 3. 章节支持折叠/展开与章节级小计。
 * 4. 新建节点可选择「章节 / 知识点」，知识点可指定归属章节。
 * 5. 知识点可在所属章节内部拖拽排序（HTML5 DnD，禁止跨章节移动）。
 */
import { ref, computed, watch, onMounted } from "vue";
import * as api from "@/api";
import Button from "@/components/ui/Button.vue";
import Badge from "@/components/ui/Badge.vue";
import Select from "@/components/ui/Select.vue";
import Checkbox from "@/components/ui/Checkbox.vue";
import Modal from "@/components/ui/Modal.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import {
  Plus,
  Trash2,
  Pencil,
  Check,
  X,
  Sparkles,
  Download,
  Upload,
  CircleDot,
  CheckCircle2,
  RotateCcw,
  BookOpen,
  ChevronRight,
  ChevronDown,
  GripVertical,
  Folder,
  FolderOpen,
  Layers,
} from "lucide-vue-next";
import type {
  ProgressTable,
  ProgressNode,
  ProgressNodeStatus,
  ProgressNodeLevel,
  ProgressWebSearchConfig,
  ProgressIndex,
} from "@/types";

const props = defineProps<{
  subject: string;
  /** 当前启用的考纲方案，如「数二」「英一」「408 计算机」 */
  variant: string;
  /** 设置中已选择的科目（来自考试类型解析）；空数组/未传 = 不做科目门控 */
  enabledSubjects?: string[];
}>();

const subjectLabelMap: Record<string, string> = {
  math: "数学",
  english: "英语",
  politics: "政治",
  professional: "专业课",
  408: "专业课",
};
const subjectLabel = computed(() => subjectLabelMap[props.subject] ?? props.subject);

// ── 数据 ──
const loading = ref(true);
const error = ref("");
/** 首屏是否已加载完成（后续刷新走静默模式，不换 DOM，避免切换表/方案时跳回顶部） */
const initialLoaded = ref(false);

const index = ref<ProgressIndex | null>(null);

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

async function reload() {
  if (!initialLoaded.value) loading.value = true;
  error.value = "";
  try {
    index.value = await api.listProgressTables();
    initialLoaded.value = true;
  } catch (e) {
    error.value = `加载进度表失败：${errMsg(e)}`;
  } finally {
    loading.value = false;
  }
}

// 当前科目的进度表集合
const subjectSet = computed(() => index.value?.subjects[props.subject]);

const allTables = computed<ProgressTable[]>(() => subjectSet.value?.tables ?? []);

/** 严格属于当前方案：variant 精确匹配 */
function tableInVariantStrict(t: ProgressTable): boolean {
  return t.variant === props.variant;
}

/** 兼容旧数据：variant 为空时，仅在当前方案为空/默认时视为匹配 */
function tableInVariantLoose(t: ProgressTable): boolean {
  if (t.variant === props.variant) return true;
  if (t.variant === "" && (!props.variant || props.variant === "默认")) return true;
  return false;
}

/** 仅当前考纲方案下的表（表下拉按方案过滤，避免 408/307 等混在一个下拉里） */
const tablesForVariant = computed<ProgressTable[]>(() =>
  allTables.value.filter(tableInVariantLoose)
);

/** 表下拉分组：内置考纲表 / 自定义表 */
const builtinTables = computed<ProgressTable[]>(() =>
  tablesForVariant.value.filter((t) => t.origin === "builtin")
);
const customTables = computed<ProgressTable[]>(() =>
  tablesForVariant.value.filter((t) => t.origin !== "builtin")
);

/** 表下拉选项文案（与旧版「X · N节点 · 启用中」一致） */
function tableOptionLabel(t: ProgressTable): string {
  return `${t.name} · ${t.nodes.length}节点${t.id === subjectSet.value?.active_id ? " · 启用中" : ""}`;
}

/** 启用表：优先精确匹配当前方案；无精确匹配时 fallback 旧数据空 variant */
const activeTable = computed<ProgressTable | null>(() => {
  const s = subjectSet.value;
  if (!s) return null;
  // 1) active_id 精确匹配当前方案
  const byActive = s.tables.find((t) => t.id === s.active_id);
  if (byActive && tableInVariantStrict(byActive)) return byActive;
  // 2) 当前方案第一张精确匹配表
  const exact = s.tables.find((t) => tableInVariantStrict(t));
  if (exact) return exact;
  // 3) fallback 旧数据
  return s.tables.find((t) => tableInVariantLoose(t)) ?? null;
});

// ── 状态元信息（5 级：待学 → 学习中 → 基础 → 强化中 → 掌握） ──
const statusMeta: Record<
  ProgressNodeStatus,
  { label: string; variant: "default" | "success" | "info" | "warning" }
> = {
  pending: { label: "待学", variant: "default" },
  learning: { label: "学习中", variant: "info" },
  basic: { label: "基础", variant: "default" },
  reinforcing: { label: "强化中", variant: "warning" },
  mastered: { label: "掌握", variant: "success" },
};

/** 点击状态胶囊：沿 待学→学习中→基础→强化中→掌握→待学 循环 */
function nextStatus(s: ProgressNodeStatus): ProgressNodeStatus {
  return s === "pending"
    ? "learning"
    : s === "learning"
      ? "basic"
      : s === "basic"
        ? "reinforcing"
        : s === "reinforcing"
          ? "mastered"
          : "pending";
}

function newNodeId(): string {
  return `n-${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
}

// ── 章节树分组 ──
interface DisplayGroup {
  /** 章节节点 id；旧表按 phase 分组时可为 null */
  headId: string | null;
  /** 章节标题 / phase */
  title: string;
  /** 组内知识点（含未分组的孤立知识点） */
  nodes: ProgressNode[];
}

const hasChapterNodes = computed(() =>
  activeTable.value?.nodes.some((n) => n.level === "chapter")
);

const groups = computed<DisplayGroup[]>(() => {
  const t = activeTable.value;
  if (!t) return [];
  const nodes = t.nodes;
  const chapters = nodes.filter((n) => n.level === "chapter");

  if (chapters.length > 0) {
    // 防御：若存在重复章节 id（旧版生成的脏数据），退化为按 phase 分组，
    // 避免所有知识点串到同一个章节下显示。
    const seenIds = new Set<string>();
    const hasDupChapterId = chapters.some((c) => {
      if (seenIds.has(c.id)) return true;
      seenIds.add(c.id);
      return false;
    });

    if (hasDupChapterId) {
      const map: Record<string, ProgressNode[]> = {};
      const order: string[] = [];
      for (const n of nodes) {
        if (n.level !== "knowledge") continue;
        const key = n.phase || "未分组";
        if (!map[key]) {
          map[key] = [];
          order.push(key);
        }
        map[key].push(n);
      }
      return order.map((phase) => ({ headId: null, title: phase, nodes: map[phase] }));
    }

    const res: DisplayGroup[] = [];
    const byId = new Map(nodes.map((n) => [n.id, n]));
    for (const ch of chapters) {
      const children = nodes.filter(
        (n) => n.level === "knowledge" && n.parent_id === ch.id
      );
      res.push({ headId: ch.id, title: ch.title, nodes: children });
    }
    // 归属不存在或未指定章节的知识点：独立成组
    const orphanNodes = nodes.filter(
      (n) =>
        n.level === "knowledge" &&
        (n.parent_id == null || !(byId.get(n.parent_id)?.level === "chapter"))
    );
    if (orphanNodes.length) {
      res.push({ headId: null, title: "未归类", nodes: orphanNodes });
    }
    return res;
  }

  // 旧表：无章节节点时按 phase 分组
  const map: Record<string, ProgressNode[]> = {};
  for (const n of nodes) {
    const key = n.phase || "未分组";
    if (!map[key]) map[key] = [];
    map[key].push(n);
  }
  return Object.keys(map).map((phase) => ({ headId: null, title: phase, nodes: map[phase] }));
});

// ── 统计（章节/知识点两级） ──
// 进度百分比按「已推进」计算：基础 / 强化中 / 掌握 都算已过完一轮基础，区别于纯待学/学习中。
const PROGRESS_STATUSES: ProgressNodeStatus[] = ["basic", "reinforcing", "mastered"];

const stats = computed(() => {
  const nodes = activeTable.value?.nodes ?? [];
  // 进度统计按「知识点」计算（章节节点仅作分组结构，不参与推进比例），
  // 与界面逐条状态胶囊、章节小计口径保持一致，避免全推进后因章节节点达标不足而显示不足 100%。
  const knowledge = nodes.filter((n) => n.level === "knowledge");
  return {
    total: knowledge.length,
    pending: knowledge.filter((n) => n.status === "pending").length,
    learning: knowledge.filter((n) => n.status === "learning").length,
    mastered: knowledge.filter((n) => n.status === "mastered").length,
    advanced: knowledge.filter((n) => PROGRESS_STATUSES.includes(n.status)).length,
    pct: knowledge.length
      ? Math.round(
          (knowledge.filter((n) => PROGRESS_STATUSES.includes(n.status)).length / knowledge.length) *
            100
        )
      : 0,
  };
});

/** 单组小计 */
function groupStats(g: DisplayGroup) {
  const advanced = g.nodes.filter((n) => PROGRESS_STATUSES.includes(n.status)).length;
  const total = g.nodes.length;
  return {
    advanced,
    total,
    pct: total ? Math.round((advanced / total) * 100) : 0,
  };
}

// ── 章节折叠 ──
const collapsedKeys = ref<string[]>([]);
function toggleGroup(g: DisplayGroup) {
  const key = g.headId ?? g.title;
  collapsedKeys.value = collapsedKeys.value.includes(key)
    ? collapsedKeys.value.filter((k) => k !== key)
    : [...collapsedKeys.value, key];
}
function isCollapsed(g: DisplayGroup): boolean {
  const key = g.headId ?? g.title;
  return collapsedKeys.value.includes(key);
}

// utils
function statusLabel(s: ProgressNodeStatus): string {
  return statusMeta[s].label;
}

function saveMsg(msg: string) {
  lastActionMsg.value = msg;
  setTimeout(() => {
    if (lastActionMsg.value === msg) lastActionMsg.value = "";
  }, 3000);
}
const lastActionMsg = ref("");

// ── 保存当前表 ──
let saving = false;
async function persistTable(table: ProgressTable, makeActive: boolean) {
  if (saving) return;
  saving = true;
  try {
    await api.saveProgressTable(props.subject, props.variant, table, makeActive);
    await reload();
  } catch (e) {
    error.value = `保存失败：${errMsg(e)}`;
  } finally {
    saving = false;
  }
}

// ── 新建表 ──
const showNewTableModal = ref(false);
const newTableName = ref("");
async function confirmNewTable() {
  const name = newTableName.value.trim();
  if (!name) return;
  const table: ProgressTable = {
    id: "",
    subject: props.subject,
    variant: props.variant,
    name,
    origin: "custom",
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    nodes: [],
  };
  await persistTable(table, true);
  newTableName.value = "";
  showNewTableModal.value = false;
  saveMsg("已创建进度表");
}

// ── 切换启用 ──
async function switchActive(id: string) {
  if (!id || id === subjectSet.value?.active_id) return;
  try {
    await api.setActiveProgressTable(props.subject, id);
    await reload();
  } catch (e) {
    error.value = `切换失败：${errMsg(e)}`;
  }
}

// ── 删除表 ──
const showDeleteTableModal = ref(false);
const deleteTableTarget = ref<ProgressTable | null>(null);
function askDeleteTable(t: ProgressTable) {
  deleteTableTarget.value = t;
  showDeleteTableModal.value = true;
}
async function confirmDeleteTable() {
  const t = deleteTableTarget.value;
  if (!t) return;
  try {
    await api.deleteProgressTable(props.subject, t.id);
    await reload();
    showDeleteTableModal.value = false;
    deleteTableTarget.value = null;
    saveMsg("已删除进度表");
  } catch (e) {
    error.value = `删除失败：${errMsg(e)}`;
  }
}

// ── 重命名表 ──
const renamingTable = ref(false);
const renameTableName = ref("");
function startRename() {
  const t = activeTable.value;
  if (!t) return;
  renameTableName.value = t.name;
  renamingTable.value = true;
}
async function confirmRename() {
  const t = activeTable.value;
  if (!t) return;
  const name = renameTableName.value.trim();
  if (!name || name === t.name) {
    renamingTable.value = false;
    return;
  }
  await persistTable({ ...t, name }, false);
  renamingTable.value = false;
  saveMsg("已重命名");
}

// ── 节点操作 ──

/** 直接落盘（节点增删/状态切换等即时改动） */
async function commitTable(mutate: (t: ProgressTable) => void) {
  const t = activeTable.value;
  if (!t) return;
  const copy: ProgressTable = { ...t, nodes: [...t.nodes] };
  mutate(copy);
  await persistTable(copy, false);
}

async function cycleStatus(node: ProgressNode) {
  await commitTable((t) => {
    const n = t.nodes.find((x) => x.id === node.id);
    if (n) n.status = nextStatus(n.status);
  });
}

async function removeNode(node: ProgressNode) {
  // 删除章节时同步删除其下知识点
  await commitTable((t) => {
    if (node.level === "chapter") {
      t.nodes = t.nodes.filter((x) => x.id !== node.id && x.parent_id !== node.id);
    } else {
      t.nodes = t.nodes.filter((x) => x.id !== node.id);
    }
  });
  if (editNodeId.value === node.id) editNodeId.value = null;
}

// ── 新建节点（可选择等级/归属章节） ──
const showAddNodeModal = ref(false);
const addNodeLevel = ref<ProgressNodeLevel>("knowledge");
const addNodeTitle = ref("");
const addNodeParentId = ref<string | null>(null);
const savingNode = ref(false);

const chapterChoices = computed(() =>
  (activeTable.value?.nodes ?? []).filter((n) => n.level === "chapter")
);

function openAddNode() {
  addNodeTitle.value = "";
  addNodeLevel.value = chapterChoices.value.length ? "knowledge" : "chapter";
  addNodeParentId.value = chapterChoices.value[0]?.id ?? null;
  showAddNodeModal.value = true;
}

async function confirmAddNode() {
  const title = addNodeTitle.value.trim();
  if (!title) return;
  savingNode.value = true;
  const level = addNodeLevel.value;
  const parentId =
    level === "knowledge" && addNodeParentId.value ? addNodeParentId.value : null;
  await commitTable((t) => {
    t.nodes.push({
      id: newNodeId(),
      title,
      level,
      parent_id: parentId,
      phase: level === "chapter" ? title : (chapterChoices.value.find((c) => c.id === parentId)?.title ?? ""),
      status: "pending",
      planned_date: null,
      note: "",
    });
  });
  savingNode.value = false;
  showAddNodeModal.value = false;
  saveMsg(level === "chapter" ? "已添加章节节点" : "已添加知识点");
}

// ── 章节内知识点拖拽排序（HTML5 DnD，禁止跨章节） ──
const draggingId = ref<string | null>(null);
const dragOverId = ref<string | null>(null);

function onDragStart(e: DragEvent, node: ProgressNode) {
  draggingId.value = node.id;
  if (e.dataTransfer) {
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", node.id);
  }
}

/** drop 到某个节点之上：仅当同属一个章节时执行重排 */
function onDragOver(e: DragEvent, target: ProgressNode) {
  e.preventDefault();
  if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
  dragOverId.value = target.id;
}

function onDrop(e: DragEvent, target: ProgressNode, group: DisplayGroup) {
  e.preventDefault();
  const draggedId = e.dataTransfer?.getData("text/plain") || draggingId.value;
  dragOverId.value = null;
  draggingId.value = null;
  if (!draggedId || draggedId === target.id) return;
  // 校验：拖拽对象必须在同一组内（跨章节禁止）
  if (!group.nodes.some((n) => n.id === draggedId)) return;
  doReorder(draggedId, target.id);
}

function onDragEnd() {
  draggingId.value = null;
  dragOverId.value = null;
}

/** 重建 flat 节点数组：将 dragged 移动到 target 之前，并保持章节/孩子顺序 */
function doReorder(draggedId: string, targetId: string) {
  const t = activeTable.value;
  if (!t) return;
  commitTable((copy) => {
    const list = [...copy.nodes];
    const from = list.findIndex((n) => n.id === draggedId);
    const to = list.findIndex((n) => n.id === targetId);
    if (from < 0 || to < 0) return;
    const [moved] = list.splice(from, 1);
    // 移除后会改变目标索引
    const toAfter = list.findIndex((n) => n.id === targetId);
    list.splice(toAfter >= 0 ? toAfter : to, 0, moved);
    copy.nodes = canonicalizeNodes(list);
  });
}

/** 将 flat 列表整理为「章节及其子树」的规范顺序，保证章节内知识点连续 */
function canonicalizeNodes(list: ProgressNode[]): ProgressNode[] {
  const chapters = list.filter((n) => n.level === "chapter");
  const childrenOf = (chId: string) => list.filter((n) => n.level === "knowledge" && n.parent_id === chId);
  const byId = new Map(list.map((n) => [n.id, n]));
  const out: ProgressNode[] = [];
  const placed = new Set<string>();
  for (const ch of chapters) {
    out.push(ch);
    placed.add(ch.id);
    for (const c of childrenOf(ch.id)) {
      if (!placed.has(c.id)) {
        out.push(c);
        placed.add(c.id);
      }
    }
  }
  // 孤立知识点/表头节点（无章节归属或归属缺失）
  for (const n of list) {
    if (placed.has(n.id)) continue;
    if (n.level === "chapter") continue; // 已在上面
    const p = n.parent_id ? byId.get(n.parent_id) : null;
    if (n.level === "knowledge" && p && p.level === "chapter") continue; // 已归属
    out.push(n);
    placed.add(n.id);
  }
  // 兜底：任何遗漏的都追加（防御异常数据）
  for (const n of list) {
    if (!placed.has(n.id)) {
      out.push(n);
      placed.add(n.id);
    }
  }
  return out;
}

// ── 内联编辑节点 ──
const editNodeId = ref<string | null>(null);
const editForm = ref<ProgressNode>(blankNode());

function blankNode(): ProgressNode {
  return {
    id: "",
    title: "",
    level: "knowledge",
    parent_id: null,
    phase: "",
    status: "pending",
    planned_date: null,
    note: "",
  };
}

function beginEdit(node: ProgressNode) {
  editNodeId.value = node.id;
  editForm.value = { ...node, planned_date: node.planned_date ?? "" };
  editFormLevel.value = node.level;
}
function cancelEdit() {
  editNodeId.value = null;
}
const editFormLevel = ref<ProgressNodeLevel>("knowledge");

async function saveEdit() {
  const id = editNodeId.value;
  if (!id) return;
  const title = editForm.value.title.trim();
  if (!title) return;
  savingNode.value = true;
  await commitTable((t) => {
    const n = t.nodes.find((x) => x.id === id);
    if (!n) return;
    n.title = title;
    n.level = editFormLevel.value;
    if (editFormLevel.value === "chapter") n.parent_id = null;
    else if (n.parent_id === null && editForm.value.parent_id) n.parent_id = editForm.value.parent_id;
    n.phase = editForm.value.phase.trim();
    n.note = editForm.value.note.trim();
    const pd = (editForm.value.planned_date as unknown) as string;
    n.planned_date = pd ? pd : null;
  });
  savingNode.value = false;
  editNodeId.value = null;
}

// ── AI 生成 ──
const showGenModal = ref(false);
const genName = ref("");
const genUseWeb = ref(false);
const generating = ref(false);
const genPreview = ref<ProgressTable | null>(null);
// AI 生成内容风险提示：首次点击「AI 生成」先弹警告，确认后本会话不再重复提醒
const showAiRiskModal = ref(false);
const aiRiskAccepted = ref(false);
const webConfig = ref<ProgressWebSearchConfig>({
  enabled: false,
  provider: "bocha",
  base_url: "",
  api_key: "",
});

function openGenModal() {
  // 首次先弹风险警告，确认后再进入配置弹窗
  if (!aiRiskAccepted.value) {
    showAiRiskModal.value = true;
    return;
  }
  genName.value = "";
  genUseWeb.value = webConfig.value.enabled;
  genPreview.value = null;
  showGenModal.value = true;
}

/** 确认风险提示：记录已接受，进入 AI 生成配置弹窗 */
function confirmAiRisk() {
  aiRiskAccepted.value = true;
  showAiRiskModal.value = false;
  genName.value = "";
  genUseWeb.value = webConfig.value.enabled;
  genPreview.value = null;
  showGenModal.value = true;
}

async function runGenerate() {
  generating.value = true;
  error.value = "";
  try {
    const draft = await api.generateProgressTable(
      props.subject,
      props.variant,
      genName.value.trim(),
      genUseWeb.value
    );
    genPreview.value = draft;
    saveMsg(genUseWeb.value && draft ? "已基于最新考纲生成（联网）" : "已基于内置考纲生成");
  } catch (e) {
    error.value = `AI 生成失败：${errMsg(e)}`;
  } finally {
    generating.value = false;
  }
}

/** 预览确认后保存为新的启用表 */
async function confirmSaveGenerated() {
  const draft = genPreview.value;
  if (!draft) return;
  await persistTable(draft, true);
  showGenModal.value = false;
  genPreview.value = null;
  saveMsg("已保存生成的进度表");
}

// ── 内置考纲：无需 AI，直接随包官方考研大纲生成进度表 ──
const loadingBuiltin = ref(false);
async function loadBuiltin() {
  if (loadingBuiltin.value) return;
  loadingBuiltin.value = true;
  error.value = "";
  try {
    const drafts = await api.builtinProgressTable(props.subject, props.variant);
    if (!drafts.length) {
      error.value = "内置考纲未生成任何进度表";
      return;
    }
    // 重新生成前清理同方案下旧的内置考纲表，避免旧数据污染/重复
    await api.deleteBuiltinProgressTables(props.subject, props.variant);
    // 专业课可能返回多份（第 1 份为总专业课进度表，其后为各教材进度表），全部入库并启用总表
    let first = true;
    for (const d of drafts) {
      try {
        await api.saveProgressTable(props.subject, props.variant, d, first);
      } catch (e) {
        error.value = `保存「${d.name}」失败：${errMsg(e)}`;
        return;
      }
      first = false;
    }
    // 全部落盘后一次性刷新索引，保证表下拉立即展示「总表 + 各教材表」（逐份保存刷新无法保证最终一致）
    await reload();
    if (drafts.length > 1) {
      saveMsg(
        `已加载内置考纲（${drafts.length} 份：总专业课进度表 + ${drafts.length - 1} 份教材进度表）`
      );
    } else {
      saveMsg(`已加载内置考纲进度表（${drafts[0].nodes.length} 个节点）`);
    }
  } catch (e) {
    error.value = `加载内置考纲失败：${errMsg(e)}`;
  } finally {
    loadingBuiltin.value = false;
  }
}

// ── 自动同步内置考纲 ──
// 需求：首次点开进度页默认展示内置考纲；切换考纲方案后若该方案还没有任何表，
// 也应自动同步内置考纲，而不是让用户手动点「内置考纲」按钮。
// 仅对已知内置考纲支持的科目生效；同一「科目|方案」全程只尝试一次，避免空转。
const BUILTIN_SUBJECTS = new Set(["math", "english", "politics", "professional", "408"]);
const autoSyncedKey = ref("");

/** 当前方案是否已有精确匹配的内置考纲表 */
const variantHasBuiltin = computed(() =>
  allTables.value.some(
    (t) => t.origin === "builtin" && tableInVariantStrict(t)
  )
);

async function autoSyncBuiltin() {
  if (!BUILTIN_SUBJECTS.has(props.subject)) return;
  // 按设置科目门控：仅在用户考试类型涉及的科目上自动同步
  if (
    props.enabledSubjects &&
    props.enabledSubjects.length > 0 &&
    !props.enabledSubjects.includes(props.subject)
  ) {
    return;
  }
  const key = `${props.subject}|${props.variant}`;
  if (autoSyncedKey.value === key) return;
  autoSyncedKey.value = key;
  if (variantHasBuiltin.value) return;
  await loadBuiltin();
  // 首次为该方案生成内置表后：协助确认实际学习进度（每科只询问一次）
  await maybeConfirmFirstProgress();
}

// ── 首次状态确认：读 State 预估进度，弹窗让用户确认/修改 ──
const showConfirmModal = ref(false);
const estimating = ref(false);
const applyingEstimate = ref(false);
const estimates = ref<api.ProgressStatusEstimate[]>([]);
/** 勾选集合：key = `${table_id}:${node_id}` */
const selectedKeys = ref<Record<string, boolean>>({});

function changeKey(e: api.ProgressStatusEstimate): string {
  return `${e.table_id}:${e.node_id}`;
}

/** 按表分组，便于弹窗内归类展示 */
const estimateGroups = computed(() => {
  const m: Record<string, api.ProgressStatusEstimate[]> = {};
  for (const e of estimates.value) {
    if (!m[e.table_name]) m[e.table_name] = [];
    m[e.table_name].push(e);
  }
  return Object.entries(m);
});

async function maybeConfirmFirstProgress() {
  // 每科只询问一次（localStorage 永久记录；即使无预估内容也标记，避免反复请求）
  const storageKey = `studyagent.progress-confirmed.${props.subject}`;
  try {
    if (localStorage.getItem(storageKey)) return;
    localStorage.setItem(storageKey, "1");
  } catch {
    // localStorage 不可用时静默跳过弹窗
    return;
  }
  estimating.value = true;
  try {
    estimates.value = await api.estimateProgressFromState(props.subject);
    if (!estimates.value.length) return;
    selectedKeys.value = {};
    for (const e of estimates.value) {
      selectedKeys.value[changeKey(e)] = true;
    }
    showConfirmModal.value = true;
  } catch {
    // 预估失败静默，不打断首开流程
  } finally {
    estimating.value = false;
  }
}

function toggleEstimate(e: api.ProgressStatusEstimate) {
  const k = changeKey(e);
  selectedKeys.value[k] = !selectedKeys.value[k];
}

async function confirmEstimate() {
  const changes: api.ProgressStatusChange[] = [];
  for (const e of estimates.value) {
    if (selectedKeys.value[changeKey(e)]) {
      changes.push({ table_id: e.table_id, node_id: e.node_id, status: e.suggested });
    }
  }
  showConfirmModal.value = false;
  if (!changes.length) return;
  applyingEstimate.value = true;
  try {
    const n = await api.applyProgressStatuses(props.subject, changes);
    await reload();
    saveMsg(`已按学习状态确认 ${n} 个知识点`);
  } catch (e) {
    error.value = `确认进度失败：${errMsg(e)}`;
  } finally {
    applyingEstimate.value = false;
  }
}

// 联网搜索配置（AI 生成时可选拉取最新大纲）
const showWebCfg = ref(false);
const savingWebCfg = ref(false);
function openWebCfg() {
  showWebCfg.value = true;
}
async function saveWebConfig() {
  savingWebCfg.value = true;
  try {
    await api.setProgressSettings(webConfig.value);
    showWebCfg.value = false;
    saveMsg("已保存联网搜索配置");
  } catch (e) {
    error.value = `保存联网配置失败：${errMsg(e)}`;
  } finally {
    savingWebCfg.value = false;
  }
}

// ── 导出 / 导入 ──
async function exportTable() {
  const t = activeTable.value;
  if (!t) return;
  try {
    const json = api.serializeProgressTableExport(props.subject, props.variant, t.name, t.nodes);
    const { save } = await import("@tauri-apps/plugin-dialog");
    const { writeTextFile } = await import("@tauri-apps/plugin-fs");
    const dest = await save({
      defaultPath: `${props.subject}-进度表-${t.name}.json`,
      filters: [{ name: "进度表", extensions: ["json"] }],
    });
    if (typeof dest !== "string") return;
    await writeTextFile(dest, json);
    saveMsg("已导出进度表");
  } catch (e) {
    error.value = `导出失败：${errMsg(e)}`;
  }
}

async function importTable() {
  let targetSubject = props.subject;
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const { readTextFile } = await import("@tauri-apps/plugin-fs");
    const file = await open({
      multiple: false,
      filters: [{ name: "进度表", extensions: ["json"] }],
    });
    if (typeof file !== "string") return;
    const raw = await readTextFile(file);
    const parsed = api.parseProgressTableExport(raw);
    // 导入到当前科目（若文件携带科目且当前未有该科集合时亦可按其科目导入）
    if (parsed.subject && parsed.subject !== targetSubject && !allTables.value.length) {
      targetSubject = parsed.subject;
    }
    const table: ProgressTable = {
      id: "",
      subject: targetSubject,
      variant: parsed.variant || props.variant,
      name: parsed.name,
      origin: "custom",
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
      nodes: parsed.nodes,
    };
    await api.saveProgressTable(
      targetSubject,
      table.variant,
      table,
      allTables.value.length === 0
    );
    saveMsg(`已导入进度表「${parsed.name}」`);
    if (targetSubject !== props.subject) {
      emit("importedOtherSubject", targetSubject);
    }
    await reload();
  } catch (e) {
    error.value = `导入失败：${errMsg(e)}`;
  }
}

const emit = defineEmits<{
  (e: "importedOtherSubject", subject: string): void;
}>();

// 方案变化时静默重载并自动同步内置考纲（active_id 已由后端对齐）
watch(
  () => props.variant,
  async () => {
    await reload();
    await autoSyncBuiltin();
  }
);

onMounted(async () => {
  await Promise.all([reload(), loadWebConfig()]);
  // 首次打开默认展示内置考纲
  await autoSyncBuiltin();
});

async function loadWebConfig() {
  try {
    webConfig.value = await api.getProgressSettings();
  } catch {
    // 忽略，保持默认
  }
}
</script>

<template>
  <div class="progress-view">
    <!-- 错误提示 -->
    <div v-if="error" class="error-banner" role="alert">
      <span class="error-text">{{ error }}</span>
      <button type="button" class="error-dismiss" aria-label="关闭错误" @click="error = ''">
        <X :size="14" />
      </button>
    </div>

    <LoadingSpinner v-if="loading" :size="28" label="加载进度表..." class="view-loading" />

    <EmptyState
      v-else-if="!activeTable"
      :title="`${subjectLabel}「${variant}」还没有进度表`"
      description="创建一份进度表，或让 AI 依据最新考纲自动生成。"
    >
      <template #actions>
        <div class="empty-actions">
          <Button
            variant="primary"
            size="sm"
            :loading="loadingBuiltin"
            @click="loadBuiltin"
            title="直接使用随包官方考研大纲生成进度表（无需 AI）"
          >
            <BookOpen :size="14" /> 内置考纲
          </Button>
          <Button variant="primary" size="sm" @click="() => (showNewTableModal = true)">
            <Plus :size="14" /> 新建进度表
          </Button>
          <Button variant="secondary" size="sm" @click="openGenModal">
            <Sparkles :size="14" /> AI 生成
          </Button>
          <Button variant="ghost" size="sm" @click="importTable">
            <Upload :size="14" /> 导入
          </Button>
        </div>
      </template>
    </EmptyState>

    <div v-else class="editor">
      <p v-if="lastActionMsg" class="action-toast">{{ lastActionMsg }}</p>

      <!-- 顶部：表切换 + 操作 -->
      <div class="toolbar">
        <div class="table-picker">
          <Badge variant="default" class="subj-badge">{{ subjectLabel }}</Badge>
          <Badge variant="info">{{ variant }}</Badge>
          <Select
            :model-value="subjectSet?.active_id ?? ''"
            placeholder="选择进度表"
            class="table-picker-select"
            @change="(v) => v != null && switchActive(String(v))"
          >
            <optgroup v-if="builtinTables.length" label="内置考纲表">
              <option v-for="t in builtinTables" :key="t.id" :value="t.id">
                {{ tableOptionLabel(t) }}
              </option>
            </optgroup>
            <optgroup v-if="customTables.length" label="自定义表">
              <option v-for="t in customTables" :key="t.id" :value="t.id">
                {{ tableOptionLabel(t) }}
              </option>
            </optgroup>
          </Select>
        </div>
        <div class="toolbar-actions">
          <Button variant="primary" size="sm" @click="openGenModal">
            <Sparkles :size="14" /> AI 生成
          </Button>
          <Button
            variant="ghost"
            size="sm"
            :loading="loadingBuiltin"
            @click="loadBuiltin"
            title="内置官方考研大纲，无需 AI 直接生成"
          >
            <BookOpen :size="14" /> 内置考纲
          </Button>
          <Button variant="ghost" size="sm" @click="() => (showNewTableModal = true)">
            <Plus :size="14" /> 新建
          </Button>
          <Button variant="ghost" size="sm" @click="openWebCfg" title="联网搜索最新考纲配置">
            <CircleDot :size="14" /> 联网
          </Button>
          <Button variant="ghost" size="sm" @click="exportTable" title="导出/分享">
            <Download :size="14" /> 导出
          </Button>
          <Button variant="ghost" size="sm" @click="importTable" title="导入">
            <Upload :size="14" /> 导入
          </Button>
        </div>
      </div>

      <!-- 表名 / renaming / 删除 -->
      <div class="table-head">
        <template v-if="renamingTable">
          <input
            v-model="renameTableName"
            class="rename-input"
            @keydown.enter="confirmRename"
            @keydown.esc="renamingTable = false"
          />
          <button class="icon-btn" title="保存" @click="confirmRename"><Check :size="14" /></button>
          <button class="icon-btn" title="取消" @click="renamingTable = false"><X :size="14" /></button>
        </template>
        <template v-else>
          <h2 class="table-name">{{ activeTable.name }}</h2>
          <button class="icon-btn" title="重命名" @click="startRename"><Pencil :size="13" /></button>
          <button class="icon-btn danger" title="删除进度表" @click="askDeleteTable(activeTable)">
            <Trash2 :size="13" />
          </button>
        </template>

        <div class="stats">
          <Badge variant="default">{{ stats.total }} 节点</Badge>
          <Badge variant="success">{{ stats.pct }}% 已推进</Badge>
          <div class="mini-bar">
            <div class="mini-bar-fill" :style="{ width: `${stats.pct}%` }" />
          </div>
        </div>
      </div>

      <!-- 章节 → 知识点 树 -->
      <div class="node-list">
        <div v-for="g in groups" :key="g.headId ?? g.title" class="node-group">
          <div class="group-head" @click="toggleGroup(g)">
            <component
              :is="isCollapsed(g) ? Folder : FolderOpen"
              :size="14"
              class="group-folder"
            />
            <span class="group-title">{{ g.title }}</span>
            <span v-if="hasChapterNodes && g.headId" class="group-level-tag">章节</span>
            <span class="group-count">
              {{ g.nodes.length }}<template v-if="g.nodes.length"> · {{ groupStats(g).advanced }}/{{ g.nodes.length }} 已推进</template>
            </span>
            <span class="mini-bar group-bar">
              <span class="mini-bar-fill" :style="{ width: `${groupStats(g).pct}%` }" />
            </span>
            <component :is="isCollapsed(g) ? ChevronRight : ChevronDown" :size="14" class="group-chev" />
          </div>

          <div v-if="!isCollapsed(g)" class="group-body">
            <div v-if="g.nodes.length === 0 && g.headId" class="group-empty">
              暂无知识点，可点击下方「添加知识点」
            </div>
            <div
              v-for="node in g.nodes"
              :key="node.id"
              class="node-row"
              :class="{ editing: editNodeId === node.id, dragging: draggingId === node.id, 'drag-over': dragOverId === node.id }"
              draggable="true"
              @dragstart="onDragStart($event, node)"
              @dragover="onDragOver($event, node)"
              @drop="onDrop($event, node, g)"
              @dragend="onDragEnd"
            >
              <template v-if="editNodeId === node.id">
                <div class="edit-grid">
                  <input v-model="editForm.title" class="edit-input edit-title" placeholder="标题" />
                  <select v-model="editFormLevel" class="edit-input">
                    <option value="chapter">章节</option>
                    <option value="knowledge">知识点</option>
                  </select>
                  <input v-model="editForm.phase" class="edit-input" placeholder="所属章节(phase)" />
                  <input v-model="editForm.planned_date" type="date" class="edit-input edit-date" />
                  <textarea v-model="editForm.note" class="edit-textarea" placeholder="备注（可选）" rows="1"></textarea>
                </div>
                <div class="edit-actions">
                  <Button variant="primary" size="sm" :loading="savingNode" @click="saveEdit">
                    <Check :size="13" /> 保存
                  </Button>
                  <Button variant="ghost" size="sm" @click="cancelEdit">取消</Button>
                </div>
              </template>

              <template v-else>
                <button class="status-pill" :class="`st-${node.status}`" title="点击切换状态" @click="cycleStatus(node)">
                  <component
                    :is="node.status === 'mastered' ? CheckCircle2 : node.status === 'learning' ? CircleDot : RotateCcw"
                    :size="13"
                  />
                  {{ statusLabel(node.status) }}
                </button>
                <div class="node-main">
                  <span class="node-title">{{ node.title }}</span>
                  <span v-if="node.planned_date" class="node-date">{{ node.planned_date }}</span>
                </div>
                <div class="node-actions">
                  <button class="icon-btn" title="编辑" @click="beginEdit(node)"><Pencil :size="13" /></button>
                  <button class="icon-btn danger" title="删除" @click="removeNode(node)"><Trash2 :size="13" /></button>
                </div>
                <GripVertical
                  :size="14"
                  class="grip"
                  draggable="true"
                  title="按住拖拽，可在章节内调整顺序"
                  @dragstart="onDragStart($event, node)"
                  @dragover="onDragOver($event, node)"
                  @drop="onDrop($event, node, g)"
                  @dragend="onDragEnd"
                />
              </template>
            </div>
          </div>
        </div>

        <div class="add-row">
          <Button variant="ghost" size="sm" @click="openAddNode">
            <Plus :size="14" /> 添加章节/知识点
          </Button>
        </div>
      </div>
    </div>

    <!-- 新建进度表 -->
    <Modal :open="showNewTableModal" title="新建进度表" :close-on-overlay="true" @close="showNewTableModal = false">
      <div class="form-field">
        <label class="form-label">进度表名称</label>
        <input v-model="newTableName" class="form-input" placeholder="如：数二全程 / 政治强化" @keydown.enter="confirmNewTable" />
        <p class="form-hint">将创建在方案「{{ variant }}」下。</p>
      </div>
      <template #footer>
        <Button variant="ghost" size="sm" @click="showNewTableModal = false">取消</Button>
        <Button variant="primary" size="sm" :disabled="!newTableName.trim()" @click="confirmNewTable">创建</Button>
      </template>
    </Modal>

    <!-- 首次状态确认：根据学习状态预估当前进度 -->
    <Modal
      :open="showConfirmModal"
      title="确认当前学习进度"
      :width="560"
      :close-on-overlay="false"
      @close="showConfirmModal = false"
    >
      <LoadingSpinner v-if="estimating" :size="24" label="正在读取学习状态..." class="view-loading" />
      <div v-else>
        <p class="form-hint">
          根据你当前的学习状态，检测到以下知识点可能已完成基础轮次。请勾选与实际相符的项
          （只升不降，已掌握/更高状态不会被降低），确认后生效。
        </p>
        <div v-for="[tableName, list] in estimateGroups" :key="tableName" class="confirm-group">
          <div class="confirm-table-name">{{ tableName }}</div>
          <label v-for="e in list" :key="changeKey(e)" class="confirm-row">
            <Checkbox :checked="!!selectedKeys[changeKey(e)]" @change="toggleEstimate(e)" />
            <span class="confirm-chapter">{{ e.chapter }}</span>
            <span class="confirm-title">{{ e.node_title }}</span>
            <span class="confirm-suggest" :class="`st-${e.suggested}`">{{ statusLabel(e.suggested) }}</span>
          </label>
        </div>
      </div>
      <template #footer>
        <Button variant="ghost" size="sm" :disabled="estimating" @click="showConfirmModal = false">
          跳过
        </Button>
        <Button
          variant="primary"
          size="sm"
          :loading="applyingEstimate"
          :disabled="estimating"
          @click="confirmEstimate"
        >
          确认进度
        </Button>
      </template>
    </Modal>

    <!-- 新建节点（选级 + 归属章节） -->
    <Modal :open="showAddNodeModal" title="添加节点" :close-on-overlay="true" @close="showAddNodeModal = false">
      <div class="form-field">
        <label class="form-label">节点等级</label>
        <div class="level-options">
          <label class="level-option" :class="{ active: addNodeLevel === 'chapter' }">
            <input type="radio" value="chapter" v-model="addNodeLevel" />
            <Folder :size="14" /> 章节
          </label>
          <label class="level-option" :class="{ active: addNodeLevel === 'knowledge' }">
            <input type="radio" value="knowledge" v-model="addNodeLevel" />
            <Layers :size="14" /> 知识点
          </label>
        </div>
      </div>
      <div class="form-field">
        <label class="form-label">标题</label>
        <input v-model="addNodeTitle" class="form-input" :placeholder="addNodeLevel === 'chapter' ? '如：第三章 微分中值定理' : '如：洛必达法则'" @keydown.enter="confirmAddNode" />
      </div>
      <div v-if="addNodeLevel === 'knowledge'" class="form-field">
        <label class="form-label">归属章节</label>
        <select v-model="addNodeParentId" class="form-input">
          <option :value="null">（不归属）</option>
          <option v-for="c in chapterChoices" :key="c.id" :value="c.id">{{ c.title }}</option>
        </select>
        <p class="form-hint">知识点新增后只能在所归属章节内拖拽排序。</p>
      </div>
      <template #footer>
        <Button variant="ghost" size="sm" @click="showAddNodeModal = false">取消</Button>
        <Button variant="primary" size="sm" :disabled="!addNodeTitle.trim()" :loading="savingNode" @click="confirmAddNode">
          添加
        </Button>
      </template>
    </Modal>

    <!-- 删除进度表 -->
    <Modal :open="showDeleteTableModal" title="删除进度表" @close="showDeleteTableModal = false">
      <p class="form-hint">确定删除「{{ deleteTableTarget?.name }}」吗？此操作不可恢复。</p>
      <template #footer>
        <Button variant="ghost" size="sm" @click="showDeleteTableModal = false">取消</Button>
        <Button variant="danger" size="sm" @click="confirmDeleteTable">删除</Button>
      </template>
    </Modal>

    <!-- AI 生成内容风险提示（首次点击 AI 生成时弹出，确认后进入配置弹窗） -->
    <Modal :open="showAiRiskModal" title="AI 生成内容风险提示" :width="560" @close="showAiRiskModal = false">
      <div class="ai-risk-box">
        <p class="ai-risk-title">⚠️ AI 生成内容风险提示</p>
        <p>本功能目前处于早期开发阶段，AI 生成的内容具有一定随机性和不确定性，可能出现包括但不限于内容过时、信息缺失、曲解课程内容、AI 幻觉等问题。</p>
        <p>此外，AI 服务通过 API 调用时，其生成效果可能与直接使用相关 AI 服务时存在差异。受模型版本、服务配置、上下文信息、可调用工具及工具权限等因素限制，AI 所能够获取和处理的信息可能受到限制，生成质量可能因此降低。同时，网络连接、API 服务状态及其他不可预见因素也可能导致无法连接、请求失败、响应异常或其他未列明的问题，从而导致生成内容不完整或不准确。</p>
        <p>由于进度表中的内容可能进一步用于规划学习任务、分配学习内容以及预估学习和复习时间，错误的 AI 生成结果可能造成严重的规划偏差，并进一步影响后续学习安排。</p>
        <p>因此，AI 生成的进度表仅供辅助参考，不应视为准确或权威的学习规划。请务必在使用前自行检查生成内容，并根据实际课程要求、教材、考试范围及个人学习情况进行核验和调整。</p>
        <p class="ai-risk-agree">使用本功能即表示你已阅读并了解上述风险。</p>
      </div>
      <template #footer>
        <Button variant="ghost" size="sm" @click="showAiRiskModal = false">取消</Button>
        <Button variant="primary" size="sm" @click="confirmAiRisk">我已了解，继续</Button>
      </template>
    </Modal>

    <!-- AI 生成 -->
    <Modal :open="showGenModal" title="AI 生成进度表" :width="520" @close="showGenModal = false">
      <LoadingSpinner v-if="generating" :size="26" label="AI 依据考纲生成中..." />
      <div v-else-if="genPreview" class="preview-box">
        <div class="preview-head">
          <Badge variant="success">{{ genPreview.nodes.length }} 个节点</Badge>
          <span v-if="genPreview.variant" class="preview-variant">{{ genPreview.variant }}</span>
          <span class="preview-name">{{ genPreview.name }}</span>
        </div>
        <ul class="preview-list">
          <li v-for="n in genPreview.nodes.slice(0, 8)" :key="n.id">
            <component :is="n.level === 'chapter' ? FolderOpen : CircleDot" :size="12" class="dot" />
            <span v-if="n.phase" class="phase-tag">{{ n.phase }}</span>
            {{ n.title }}
          </li>
          <li v-if="genPreview.nodes.length > 8" class="more">… 还有 {{ genPreview.nodes.length - 8 }} 个节点</li>
        </ul>
        <div class="preview-actions">
          <Button variant="ghost" size="sm" @click="genPreview = null">重新生成</Button>
          <Button variant="primary" size="sm" @click="confirmSaveGenerated">保存为启用进度表</Button>
        </div>
      </div>
      <div v-else class="gen-form">
        <div class="form-field">
          <label class="form-label">进度表名称</label>
          <input v-model="genName" class="form-input" placeholder="留空则自动命名，如「数学进度表」" />
        </div>
        <label class="web-toggle">
          <Checkbox v-model="genUseWeb" />
          <span>联网查询最新考研大纲（未配置使用内置考纲）</span>
        </label>
        <p class="form-hint">将依据 {{ subjectLabel }}「{{ variant }}」的最新考研考纲，按章节先后顺序生成可供长期打卡的进度节点。</p>
      </div>
      <template #footer v-if="!generating && !genPreview">
        <Button variant="ghost" size="sm" @click="showGenModal = false">取消</Button>
        <Button variant="primary" size="sm" :loading="generating" @click="runGenerate">
          <Sparkles :size="14" /> 生成
        </Button>
      </template>
    </Modal>

    <!-- 联网搜索配置 -->
    <Modal :open="showWebCfg" title="联网搜索最新考研大纲" :width="460" @close="showWebCfg = false">
      <div class="form-field">
        <label class="web-toggle">
          <Checkbox v-model="webConfig.enabled" />
          <span>启用联网搜索</span>
        </label>
      </div>
      <div class="form-field">
        <label class="form-label">厂商</label>
        <input v-model="webConfig.provider" class="form-input" disabled />
      </div>
      <div class="form-field">
        <label class="form-label">API Base URL</label>
        <input v-model="webConfig.base_url" class="form-input" placeholder="留空使用博查查默认 https://api.bochaai.com/v1/web-search" />
      </div>
      <div class="form-field">
        <label class="form-label">API Key</label>
        <input v-model="webConfig.api_key" type="password" class="form-input" placeholder="粘贴博查查 API Key" />
      </div>
      <p class="form-hint">启用后，AI 生成进度表会先联网检索最新考研大纲；未配置或检索失败时自动回退内置官方考纲。</p>
      <template #footer>
        <Button variant="ghost" size="sm" @click="showWebCfg = false">取消</Button>
        <Button variant="primary" size="sm" :loading="savingWebCfg" @click="saveWebConfig">保存</Button>
      </template>
    </Modal>
  </div>
</template>

<style scoped>
.progress-view {
  position: relative;
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}
.view-loading {
  margin: auto;
}

/* AI 生成内容风险提示弹窗 */
.ai-risk-box {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  max-height: 46vh;
  overflow-y: auto;
}
.ai-risk-box p {
  margin: 0;
  font-size: var(--text-sm);
  line-height: var(--leading-relaxed);
  color: var(--text-secondary);
}
.ai-risk-title {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--color-warning, #b45309);
}
.ai-risk-agree {
  font-weight: var(--font-medium);
  color: var(--text-primary);
}
.error-banner {
  position: absolute;
  top: var(--space-2);
  left: 50%;
  transform: translateX(-50%);
  z-index: 20;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  max-width: calc(100% - 24px);
  padding: 6px 10px;
  border-radius: 8px;
  background: var(--color-danger-subtle);
  border: 1px solid var(--color-danger);
  color: var(--color-danger);
  font-size: 12px;
  box-shadow: 0 2px 10px rgba(0, 0, 0, 0.12);
}
.error-text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.error-dismiss { display: flex; align-items: center; justify-content: center; padding: 2px; border: none; background: transparent; color: inherit; cursor: pointer; }
.action-toast {
  position: absolute;
  top: var(--space-2);
  left: 50%;
  transform: translateX(-50%);
  z-index: 9;
  padding: 6px 12px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  font-size: var(--text-xs);
  color: var(--text-secondary);
  box-shadow: var(--shadow-sm);
}
.empty-actions {
  display: flex;
  gap: var(--space-2);
  flex-wrap: wrap;
  justify-content: center;
}
.editor {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: var(--space-4) var(--space-6) var(--space-10);
}
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  flex-wrap: wrap;
  margin-bottom: var(--space-3);
}
.table-picker { display: flex; align-items: center; gap: var(--space-2); min-width: 0; }
.subj-badge { flex-shrink: 0; }
/* 表下拉按内容自适应宽度（默认 100% 会让它占满整行，覆盖为 auto 并给最小/最大边界） */
.table-picker .table-picker-select {
  width: auto;
  min-width: 200px;
  max-width: 100%;
}
.toolbar-actions { display: flex; gap: var(--space-1); flex-wrap: wrap; }
.table-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-wrap: wrap;
  margin-bottom: var(--space-4);
  padding-bottom: var(--space-3);
  border-bottom: 1px solid var(--divider-color);
}
.table-name { font-size: var(--text-xl); font-weight: var(--font-bold); margin: 0; color: var(--text-primary); }
.rename-input {
  height: 32px;
  padding: 0 var(--space-3);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--bg-primary);
  color: var(--text-primary);
  font-family: inherit;
  font-size: var(--text-base);
  max-width: 320px;
}
.icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  border-radius: var(--radius-xs);
  cursor: pointer;
  transition: all var(--transition-fast);
}
.icon-btn:hover { background: var(--bg-tertiary); color: var(--text-primary); }
.icon-btn.danger:hover { color: var(--color-danger); }
.stats {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.mini-bar {
  width: 120px;
  height: 6px;
  border-radius: var(--radius-full);
  background: var(--bg-tertiary);
  overflow: hidden;
}
.mini-bar-fill { height: 100%; background: var(--color-success, #22c55e); transition: width 0.3s ease; }

.node-list { display: flex; flex-direction: column; gap: var(--space-3); }
.node-group {
  display: flex;
  flex-direction: column;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  overflow: hidden;
}
.group-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  cursor: pointer;
  user-select: none;
  background: var(--bg-tertiary);
}
.group-head:hover { background: var(--bg-tertiary); }
.group-folder { color: var(--accent); flex-shrink: 0; }
.group-title { font-size: var(--text-sm); font-weight: var(--font-semibold); color: var(--text-primary); flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.group-level-tag {
  flex-shrink: 0;
  font-size: 10px;
  color: var(--accent);
  background: var(--accent-subtle);
  border-radius: var(--radius-xs);
  padding: 1px 6px;
}
.group-count { font-size: var(--text-xs); color: var(--text-tertiary); flex-shrink: 0; }
.group-bar { width: 80px; flex-shrink: 0; }
.group-chev { color: var(--text-tertiary); flex-shrink: 0; }
.group-body { display: flex; flex-direction: column; gap: 2px; padding: var(--space-1); }
.group-empty { padding: var(--space-3); font-size: var(--text-xs); color: var(--text-tertiary); text-align: center; }
.node-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-md);
  border: 1px solid transparent;
  transition: border-color var(--transition-fast), opacity var(--transition-fast);
}
.node-row:hover { border-color: var(--border-color-strong); }
.node-row.editing { border-color: var(--accent); }
.node-row.drag-over { border-color: var(--accent); background: var(--accent-subtle); }
.node-row.dragging { opacity: 0.4; }
.grip { color: var(--text-quaternary); cursor: grab; flex-shrink: 0; }
.grip:active { cursor: grabbing; }
.grip:hover { color: var(--text-secondary); }
.status-pill {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  flex-shrink: 0;
  padding: 2px 8px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-full);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--text-xs);
  cursor: pointer;
  transition: all var(--transition-fast);
}
.status-pill:hover { transform: scale(1.04); }
.st-pending { color: var(--text-tertiary); }
.st-learning { color: var(--accent); border-color: var(--accent-soft, var(--border-color)); background: var(--accent-subtle); }
.st-basic { color: var(--info-color, #0284c7); border-color: var(--info-color-soft, var(--border-color)); background: var(--info-subtle, transparent); }
.st-reinforcing { color: var(--warning-color, #d97706); border-color: var(--warning-color-soft, var(--border-color)); background: var(--warning-subtle, transparent); }
.st-mastered { color: var(--color-success, #16a34a); border-color: var(--color-success-soft, var(--border-color)); }
.node-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
.node-title { font-size: var(--text-sm); font-weight: var(--font-medium); color: var(--text-primary); word-break: break-word; }
.node-date { font-size: var(--text-xs); color: var(--text-tertiary); }
.node-actions { display: flex; gap: 2px; flex-shrink: 0; opacity: 0; transition: opacity var(--transition-fast); }
.node-row:hover .node-actions, .node-row.editing .node-actions { opacity: 1; }
.edit-grid { flex: 1; display: grid; grid-template-columns: 1fr 1fr; gap: var(--space-2); }
.edit-title { grid-column: 1 / -1; }
.edit-date { max-width: 150px; }
.edit-input, .edit-textarea {
  padding: 6px 10px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
  color: var(--text-primary);
  font-family: inherit;
  font-size: var(--text-sm);
  outline: none;
}
.edit-input:focus, .edit-textarea:focus { border-color: var(--accent); }
.edit-textarea { resize: vertical; }
.edit-actions { display: flex; gap: var(--space-1); flex-shrink: 0; }
.add-row { padding: var(--space-2) 0; }

/* 表单 */
.form-field { display: flex; flex-direction: column; gap: var(--space-1); }
.form-label { font-size: var(--text-sm); font-weight: var(--font-medium); color: var(--text-secondary); }
.form-input {
  height: 36px;
  padding: 0 var(--space-3);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--bg-primary);
  color: var(--text-primary);
  font-family: inherit;
  font-size: var(--text-sm);
  outline: none;
}
.form-input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-subtle); }
.form-hint { margin: 0; font-size: var(--text-xs); color: var(--text-tertiary); line-height: var(--leading-normal); }
.confirm-group { display: flex; flex-direction: column; gap: 2px; margin-bottom: var(--space-3); }
.confirm-table-name { font-size: var(--text-xs); font-weight: var(--font-semibold); color: var(--text-tertiary); padding: 4px 0 2px; }
.confirm-row { display: flex; align-items: center; gap: var(--space-2); padding: 4px 6px; border-radius: var(--radius-sm); cursor: pointer; }
.confirm-row:hover { background: var(--bg-tertiary); }
.confirm-chapter { font-size: var(--text-xs); color: var(--text-tertiary); flex-shrink: 0; max-width: 150px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.confirm-title { font-size: var(--text-sm); color: var(--text-primary); flex: 1; min-width: 0; }
.confirm-suggest { font-size: var(--text-xs); border: 1px solid var(--border-color); border-radius: var(--radius-full); padding: 1px 6px; flex-shrink: 0; }
.web-toggle { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-sm); color: var(--text-secondary); cursor: pointer; }
.gen-form { display: flex; flex-direction: column; gap: var(--space-4); }
.preview-box {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}
.preview-head { display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap; }
.preview-variant {
  font-size: 11px;
  color: var(--accent);
  background: var(--accent-subtle);
  border-radius: var(--radius-xs);
  padding: 1px 6px;
}
.preview-name { font-weight: var(--font-semibold); color: var(--text-primary); }
.preview-list { margin: 0; padding: 0; list-style: none; display: flex; flex-direction: column; gap: var(--space-1); max-height: 300px; overflow-y: auto; }
.preview-list li { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-sm); color: var(--text-secondary); }
.preview-list .dot { color: var(--text-tertiary); flex-shrink: 0; }
.phase-tag {
  flex-shrink: 0;
  font-size: 11px;
  color: var(--accent);
  background: var(--accent-subtle);
  border-radius: var(--radius-xs);
  padding: 1px 6px;
}
.preview-list .more { color: var(--text-tertiary); }
.preview-actions { display: flex; justify-content: flex-end; gap: var(--space-2); }

/* 节点等级选择 */
.level-options { display: flex; gap: var(--space-2); }
.level-option {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex: 1;
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  color: var(--text-secondary);
  cursor: pointer;
  transition: all var(--transition-fast);
}
.level-option.active { border-color: var(--accent); color: var(--accent); background: var(--accent-subtle); }
.level-option input { display: none; }
</style>