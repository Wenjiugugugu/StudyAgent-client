<script setup lang="ts">
/** 各科「进度表」编辑器：多表切换/启用、节点增删改、状态打卡、AI 生成、导出/导入 */
import { ref, computed, onMounted } from "vue";
import * as api from "@/api";
import Button from "@/components/ui/Button.vue";
import Badge from "@/components/ui/Badge.vue";
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
} from "lucide-vue-next";
import { defineComponent, h } from "vue";
import type {
  ProgressTable,
  ProgressNode,
  ProgressNodeStatus,
  ProgressWebSearchConfig,
  ProgressIndex,
} from "@/types";

/** 简易“表”选择器：用原生 select 展示多表并切换启用 */
const SelectLikeList = defineComponent({
  props: {
    tables: { type: Array as () => ProgressTable[], default: () => [] },
    activeId: { type: String, default: "" },
  },
  emits: ["switch"],
  setup(props, { emit }) {
    return () =>
      h(
        "select",
        {
          class: "table-select",
          value: props.activeId,
          onChange: (e: Event) => {
            const v = (e.target as HTMLSelectElement).value;
            emit("switch", v);
          },
        },
        props.tables.map((t) =>
          h(
            "option",
            { key: t.id, value: t.id, selected: t.id === props.activeId },
            `${t.name} · ${t.nodes.length}节点${t.id === props.activeId ? " · 启用中" : ""}`
          )
        )
      );
  },
});

const props = defineProps<{
  subject: string;
  examType: string;
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

const index = ref<ProgressIndex | null>(null);

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

async function reload() {
  loading.value = true;
  error.value = "";
  try {
    index.value = await api.listProgressTables();
  } catch (e) {
    error.value = `加载进度表失败：${errMsg(e)}`;
  } finally {
    loading.value = false;
  }
}

// 当前科目的进度表集合
const subjectSet = computed(() => index.value?.subjects[props.subject]);

const allTables = computed<ProgressTable[]>(() => subjectSet.value?.tables ?? []);

const activeTable = computed<ProgressTable | null>(() => {
  const s = subjectSet.value;
  if (!s) return null;
  return (
    s.tables.find((t) => t.id === s.active_id) ??
    s.tables[0] ??
    null
  );
});

// ── 状态元信息 ──
const statusMeta: Record<
  ProgressNodeStatus,
  { label: string; variant: "default" | "success" | "info" | "warning" }
> = {
  pending: { label: "待学", variant: "default" },
  learning: { label: "学习中", variant: "info" },
  mastered: { label: "已掌握", variant: "success" },
};

function nextStatus(s: ProgressNodeStatus): ProgressNodeStatus {
  return s === "pending" ? "learning" : s === "learning" ? "mastered" : "pending";
}

function newNodeId(): string {
  return `n-${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
}
function newTableId(): string {
  return `p-${Date.now()}`;
}

// ── 统计 ──
const stats = computed(() => {
  const nodes = activeTable.value?.nodes ?? [];
  return {
    total: nodes.length,
    pending: nodes.filter((n) => n.status === "pending").length,
    learning: nodes.filter((n) => n.status === "learning").length,
    mastered: nodes.filter((n) => n.status === "mastered").length,
    pct: nodes.length
      ? Math.round((nodes.filter((n) => n.status === "mastered").length / nodes.length) * 100)
      : 0,
  };
});

const groupedNodes = computed(() => {
  const table = activeTable.value;
  if (!table) return [] as { phase: string; list: ProgressNode[] }[];
  const map: Record<string, ProgressNode[]> = {};
  for (const n of table.nodes) {
    const key = n.phase || "未分组";
    if (!map[key]) map[key] = [];
    map[key].push(n);
  }
  return Object.keys(map).map((phase) => ({ phase, list: map[phase] }));
});

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
    await api.saveProgressTable(props.subject, table, makeActive);
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
    id: newTableId(),
    subject: props.subject,
    name,
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

async function addNode() {
  await commitTable((t) => {
    t.nodes.push({
      id: newNodeId(),
      title: "新知识点",
      phase: "",
      status: "pending",
      planned_date: null,
      note: "",
    });
  });
  // 展开新节点编辑
  const t = activeTable.value;
  if (t) editNodeId.value = t.nodes[t.nodes.length - 1]?.id ?? null;
  saveMsg("已添加节点，点击编辑填写内容");
}

async function removeNode(node: ProgressNode) {
  await commitTable((t) => {
    t.nodes = t.nodes.filter((x) => x.id !== node.id);
  });
  if (editNodeId.value === node.id) editNodeId.value = null;
}

// ── 内联编辑节点 ──
const editNodeId = ref<string | null>(null);
const editForm = ref<ProgressNode>({ ...blankNode() });
const savingNode = ref(false);

function blankNode(): ProgressNode {
  return { id: "", title: "", phase: "", status: "pending", planned_date: null, note: "" };
}

function beginEdit(node: ProgressNode) {
  editNodeId.value = node.id;
  editForm.value = { ...node, planned_date: node.planned_date ?? "" };
}
function cancelEdit() {
  editNodeId.value = null;
}
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
const webConfig = ref<ProgressWebSearchConfig>({
  enabled: false,
  provider: "bocha",
  base_url: "",
  api_key: "",
});

function openGenModal() {
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
      props.examType,
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
  await persistTable({ ...draft, id: newTableId() }, true);
  showGenModal.value = false;
  genPreview.value = null;
  saveMsg("已保存生成的进度表");
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
    const json = api.serializeProgressTableExport(props.subject, t.name, t.nodes);
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
      id: newTableId(),
      subject: targetSubject,
      name: parsed.name,
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
      nodes: parsed.nodes,
    };
    await api.saveProgressTable(targetSubject, table, allTables.value.length === 0);
    saveMsg(`已导入进度表「${parsed.name}」`);
    if (targetSubject !== props.subject) {
      // 导入到了其它科目，通知父组件刷新该科入口
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

onMounted(async () => {
  await Promise.all([reload(), loadWebConfig()]);
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
      :title="`${subjectLabel}还没有进度表`"
      description="创建一份进度表，或让 AI 依据最新考纲自动生成。"
    >
      <template #actions>
        <div class="empty-actions">
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
          <SelectLikeList
            :tables="allTables"
            :activeId="subjectSet!.active_id"
            @switch="switchActive"
          />
        </div>
        <div class="toolbar-actions">
          <Button variant="primary" size="sm" @click="openGenModal">
            <Sparkles :size="14" /> AI 生成
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
          <Badge variant="default">{{ stats.pct }}% 掌握</Badge>
          <div class="mini-bar">
            <div class="mini-bar-fill" :style="{ width: `${stats.pct}%` }" />
          </div>
        </div>
      </div>

      <!-- 节点列表（按 phase 分组） -->
      <div class="node-list">
        <div v-for="g in groupedNodes" :key="g.phase" class="node-group">
          <div class="group-head">
            <span class="group-title">{{ g.phase }}</span>
            <span class="group-count">{{ g.list.length }}</span>
          </div>
          <div class="group-body">
            <div v-for="node in g.list" :key="node.id" class="node-row" :class="{ editing: editNodeId === node.id }">
              <template v-if="editNodeId === node.id">
                <div class="edit-grid">
                  <input v-model="editForm.title" class="edit-input edit-title" placeholder="知识点标题" />
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
              </template>
            </div>
          </div>
        </div>

        <div class="add-row">
          <Button variant="ghost" size="sm" @click="addNode">
            <Plus :size="14" /> 添加节点
          </Button>
        </div>
      </div>
    </div>

    <!-- 新建进度表 -->
    <Modal :open="showNewTableModal" title="新建进度表" :close-on-overlay="true" @close="showNewTableModal = false">
      <div class="form-field">
        <label class="form-label">进度表名称</label>
        <input v-model="newTableName" class="form-input" placeholder="如：数二全程 / 政治强化" @keydown.enter="confirmNewTable" />
      </div>
      <template #footer>
        <Button variant="ghost" size="sm" @click="showNewTableModal = false">取消</Button>
        <Button variant="primary" size="sm" :disabled="!newTableName.trim()" @click="confirmNewTable">创建</Button>
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

    <!-- AI 生成 -->
    <Modal :open="showGenModal" title="AI 生成进度表" :width="520" @close="showGenModal = false">
      <LoadingSpinner v-if="generating" :size="26" label="AI 依据考纲生成中..." />
      <div v-else-if="genPreview" class="preview-box">
        <div class="preview-head">
          <Badge variant="success">{{ genPreview.nodes.length }} 个节点</Badge>
          <span class="preview-name">{{ genPreview.name }}</span>
        </div>
        <ul class="preview-list">
          <li v-for="n in genPreview.nodes.slice(0, 8)" :key="n.id">
            <CircleDot :size="12" class="dot" />
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
          <input v-model="genUseWeb" type="checkbox" />
          <span>联网查询最新考研大纲（未配置使用内置考纲）</span>
        </label>
        <p class="form-hint">将依据{{ subjectLabel }}的最新考研考纲，按章节先后顺序生成可供长期打卡的进度节点。</p>
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
          <input v-model="webConfig.enabled" type="checkbox" />
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
.table-picker .table-select {
  max-width: 260px;
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

.node-list { display: flex; flex-direction: column; gap: var(--space-4); }
.node-group { display: flex; flex-direction: column; gap: 2px; }
.group-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--text-tertiary);
  padding: var(--space-1) var(--space-2);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.group-count { color: var(--text-quaternary); }
.group-body { display: flex; flex-direction: column; gap: 2px; }
.node-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  transition: border-color var(--transition-fast);
}
.node-row:hover { border-color: var(--border-color-strong); }
.node-row.editing { border-color: var(--accent); }
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
.table-select {
  height: 32px;
  padding: 0 var(--space-3);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  color: var(--text-primary);
  font-family: inherit;
  font-size: var(--text-sm);
}

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
.web-toggle { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-sm); color: var(--text-secondary); cursor: pointer; }
.gen-form { display: flex; flex-direction: column; gap: var(--space-4); }
.preview-box {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}
.preview-head { display: flex; align-items: center; gap: var(--space-2); }
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
</style>