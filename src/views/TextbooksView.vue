<script setup lang="ts">
import { ref, computed, onMounted, nextTick } from "vue";
import * as api from "@/api";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import {
  Search,
  BookOpen,
  List,
  Hash,
  Plus,
  Trash2,
  FileSearch,
  PanelLeft,
  PanelLeftClose,
  Pencil,
} from "lucide-vue-next";
import type { TextbookInfo, TextbookContent, TextbookSearchHit } from "@/types";

// ── 数据 ──
const textbooks = ref<TextbookInfo[]>([]);
const loadingList = ref(false);
const current = ref<TextbookContent | null>(null);
const loadingContent = ref(false);
const currentMeta = ref<TextbookInfo | null>(null);

// ── 搜索模式：list（教材列表过滤） / content（全文搜索） ──
type SearchMode = "list" | "content";
const searchMode = ref<SearchMode>("list");
const searchQuery = ref("");
const searchHits = ref<TextbookSearchHit[]>([]);
const searching = ref(false);

// ── 列表折叠 ──
const listCollapsed = ref(false);
function toggleList() {
  listCollapsed.value = !listCollapsed.value;
}

// ── 导入教材对话框 ──
const showImportDialog = ref(false);
const importSubject = ref<string>("math");
const importTitle = ref<string>("");
const importing = ref(false);

// ── 重命名教材对话框 ──
const showRenameDialog = ref(false);
const renameTarget = ref<TextbookInfo | null>(null);
const renameNewTitle = ref<string>("");
const renaming = ref(false);
const renameInput = ref<HTMLInputElement | null>(null);

const subjectOptions = [
  { value: "math", label: "数学" },
  { value: "english", label: "英语" },
  { value: "politics", label: "政治" },
  { value: "professional", label: "专业课" },
];

// ── 学科分组 ──
const groupedTextbooks = computed(() => {
  const q = searchQuery.value.trim().toLowerCase();
  const filtered = q
    ? textbooks.value.filter(
        (t) =>
          t.title.toLowerCase().includes(q) ||
          t.subject.toLowerCase().includes(q) ||
          t.filename.toLowerCase().includes(q)
      )
    : textbooks.value;

  const groups: Record<string, TextbookInfo[]> = {};
  for (const t of filtered) {
    if (!groups[t.subject]) groups[t.subject] = [];
    groups[t.subject].push(t);
  }
  return Object.entries(groups).map(([subject, items]) => ({
    subject,
    items,
  }));
});

// ── 全文搜索 ──
async function runContentSearch() {
  const q = searchQuery.value.trim();
  if (!q) {
    searchHits.value = [];
    return;
  }
  searching.value = true;
  try {
    searchHits.value = await api.searchInTextbook(q);
  } catch (e) {
    searchHits.value = [];
  } finally {
    searching.value = false;
  }
}

let searchDebounce: ReturnType<typeof setTimeout> | null = null;
function onSearchInput() {
  if (searchMode.value !== "content") return;
  if (searchDebounce) clearTimeout(searchDebounce);
  searchDebounce = setTimeout(() => {
    runContentSearch();
  }, 300);
}

function switchMode(mode: SearchMode) {
  searchMode.value = mode;
  searchHits.value = [];
  if (mode === "content" && searchQuery.value.trim()) {
    runContentSearch();
  }
}

function jumpToSearchHit(hit: TextbookSearchHit) {
  selectTextbook({
    id: hit.textbook_id,
    subject: hit.subject,
    title: hit.textbook_title,
    filename: "",
    file_path: "",
  } as TextbookInfo).then(() => {
    // 延迟滚动到匹配行。H39：改用 data-line 属性精确定位，替代脆弱的 DOM 行索引
    nextTick(() => {
      setTimeout(() => {
        const reader = document.querySelector(".textbook-reader");
        if (reader) {
          const target = reader.querySelector(`[data-line="${hit.line_number}"]`);
          if (target) {
            (target as HTMLElement).scrollIntoView({
              behavior: "smooth",
              block: "center",
            });
          } else {
            // 未找到精确行号时的回退：定位到最近的段落
            const fallback = reader.querySelector(
              `[data-line="${hit.line_number - 1}"], [data-line="${hit.line_number + 1}"]`
            );
            if (fallback) {
              (fallback as HTMLElement).scrollIntoView({
                behavior: "smooth",
                block: "center",
              });
            }
          }
        }
      }, 200);
    });
  });
}

// ── 导入教材 ──
function openImportDialog() {
  importSubject.value = "math";
  importTitle.value = "";
  showImportDialog.value = true;
}

async function handleImport() {
  importing.value = true;
  try {
    await api.importTextbook(importSubject.value, importTitle.value || undefined);
    showImportDialog.value = false;
    await loadTextbooks();
  } catch (e) {
    if (e instanceof Error && e.message !== "未选择文件") {
      alert(`导入教材失败：${e.message}`);
    }
  } finally {
    importing.value = false;
  }
}

// ── 删除教材 ──
async function handleDelete(t: TextbookInfo, e: Event) {
  e.stopPropagation();
  if (!confirm(`确定删除教材《${t.title}》吗？此操作不可恢复。`)) return;
  try {
    await api.deleteTextbook(t.id);
    if (currentMeta.value?.id === t.id) {
      current.value = null;
      currentMeta.value = null;
    }
    await loadTextbooks();
  } catch (err) {
    alert(`删除失败：${err instanceof Error ? err.message : String(err)}`);
  }
}

// ── 重命名教材 ──
function openRenameDialog(t: TextbookInfo, e: Event) {
  e.stopPropagation();
  renameTarget.value = t;
  renameNewTitle.value = t.title;
  showRenameDialog.value = true;
  nextTick(() => {
    renameInput.value?.focus();
    renameInput.value?.select();
  });
}

function closeRenameDialog() {
  showRenameDialog.value = false;
  renameTarget.value = null;
  renameNewTitle.value = "";
  renaming.value = false;
}

async function handleRename() {
  const t = renameTarget.value;
  if (!t) return;
  const trimmed = renameNewTitle.value.trim();
  if (!trimmed) {
    alert("教材标题不能为空");
    return;
  }
  if (trimmed === t.title) {
    closeRenameDialog();
    return;
  }
  renaming.value = true;
  try {
    const updated = await api.renameTextbook(t.id, trimmed);
    // 如果当前正在阅读这本教材，同步更新 meta
    if (currentMeta.value?.id === t.id) {
      currentMeta.value = updated;
    }
    await loadTextbooks();
    closeRenameDialog();
  } catch (err) {
    alert(`重命名失败：${err instanceof Error ? err.message : String(err)}`);
  } finally {
    renaming.value = false;
  }
}

// ── 目录（解析 Markdown 标题） ──
interface TocItem {
  level: number;
  text: string;
  id: string;
}
const toc = computed<TocItem[]>(() => {
  if (!current.value) return [];
  const lines = current.value.content.split("\n");
  const items: TocItem[] = [];
  let inCodeBlock = false;
  for (const line of lines) {
    if (line.trim().startsWith("```")) {
      inCodeBlock = !inCodeBlock;
      continue;
    }
    if (inCodeBlock) continue;
    const match = line.match(/^(#{1,6})\s+(.+)$/);
    if (match) {
      const level = match[1].length;
      const text = match[2].trim();
      items.push({ level, text, id: slugify(text) });
    }
  }
  return items;
});

const activeTocId = ref<string | null>(null);

// ── 简单 Markdown 渲染 ──
const renderedHtml = computed(() => {
  if (!current.value) return "";
  return renderMarkdown(current.value.content);
});

function slugify(text: string): string {
  return (
    "tb-" +
    text
      .toLowerCase()
      .replace(/[^\w\u4e00-\u9fa5]+/g, "-")
      .replace(/^-+|-+$/g, "")
  );
}

/**
 * 简单 Markdown 渲染器
 * 支持：标题、段落、有序/无序列表、加粗、行内代码、代码块、链接、引用
 */
function escapeHtml(str: string): string {
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function renderInline(text: string): string {
  let s = escapeHtml(text);
  // 行内代码 `code`
  s = s.replace(/`([^`]+)`/g, '<code class="md-code">$1</code>');
  // 加粗 **text**
  s = s.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  // 斜体 *text*（避免与加粗冲突）
  s = s.replace(/(^|[^*])\*([^*]+)\*(?!\*)/g, "$1<em>$2</em>");
  // 链接 [text](url)。C8：仅允许 http/https/mailto scheme，拒绝 javascript:/data:/vbscript: 等
  s = s.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_m, label: string, url: string) => {
    const trimmedUrl = url.trim();
    const allowed = /^(https?:\/\/|mailto:)/i.test(trimmedUrl);
    if (!allowed) {
      // 不安全的 URL：渲染为纯文本而非可点击链接
      return `${label}`;
    }
    return `<a href="${trimmedUrl}" target="_blank" rel="noopener" class="md-link">${label}</a>`;
  });
  return s;
}

function renderMarkdown(src: string): string {
  const lines = src.replace(/\r\n/g, "\n").split("\n");
  const out: string[] = [];
  let i = 0;
  let inCodeBlock = false;
  let codeLang = "";
  let codeBuffer: string[] = [];
  let listType: "ul" | "ol" | null = null;

  // H39：为每个块元素嵌入源行号（1-based），供搜索命中精确跳转，替代脆弱的 DOM 行索引
  const lineAttr = () => ` data-line="${i + 1}"`;

  const closeList = () => {
    if (listType) {
      out.push(`</${listType}>`);
      listType = null;
    }
  };

  while (i < lines.length) {
    const line = lines[i];

    // 代码块围栏
    const fence = line.match(/^```(\w*)\s*$/);
    if (fence) {
      if (!inCodeBlock) {
        closeList();
        inCodeBlock = true;
        codeLang = fence[1] || "";
        codeBuffer = [];
      } else {
        const code = escapeHtml(codeBuffer.join("\n"));
        out.push(
          `<pre${lineAttr()} class="md-pre"><code class="md-block-code${codeLang ? ` language-${codeLang}` : ""}">${code}</code></pre>`
        );
        inCodeBlock = false;
        codeLang = "";
      }
      i++;
      continue;
    }

    if (inCodeBlock) {
      codeBuffer.push(line);
      i++;
      continue;
    }

    // 空行
    if (line.trim() === "") {
      closeList();
      i++;
      continue;
    }

    // 标题
    const heading = line.match(/^(#{1,6})\s+(.+)$/);
    if (heading) {
      closeList();
      const level = heading[1].length;
      const text = heading[2].trim();
      const id = slugify(text);
      out.push(
        `<h${level}${lineAttr()} id="${id}" class="md-h md-h${level}">${renderInline(text)}</h${level}>`
      );
      i++;
      continue;
    }

    // 引用
    const blockquote = line.match(/^>\s?(.*)$/);
    if (blockquote) {
      closeList();
      out.push(`<blockquote${lineAttr()} class="md-quote">${renderInline(blockquote[1])}</blockquote>`);
      i++;
      continue;
    }

    // 无序列表
    const ulItem = line.match(/^[-*+]\s+(.+)$/);
    if (ulItem) {
      if (listType !== "ul") {
        closeList();
        out.push('<ul class="md-ul">');
        listType = "ul";
      }
      out.push(`<li${lineAttr()}>${renderInline(ulItem[1])}</li>`);
      i++;
      continue;
    }

    // 有序列表
    const olItem = line.match(/^\d+\.\s+(.+)$/);
    if (olItem) {
      if (listType !== "ol") {
        closeList();
        out.push('<ol class="md-ol">');
        listType = "ol";
      }
      out.push(`<li${lineAttr()}>${renderInline(olItem[1])}</li>`);
      i++;
      continue;
    }

    // 水平线
    if (/^(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      closeList();
      out.push(`<hr${lineAttr()} class="md-hr" />`);
      i++;
      continue;
    }

    // 段落（普通文本）
    closeList();
    out.push(`<p${lineAttr()} class="md-p">${renderInline(line.trim())}</p>`);
    i++;
  }

  closeList();
  if (inCodeBlock) {
    // 未闭合的代码块
    const code = escapeHtml(codeBuffer.join("\n"));
    out.push(`<pre class="md-pre"><code class="md-block-code">${code}</code></pre>`);
  }
  return out.join("\n");
}

// ── 加载 ──
async function loadTextbooks() {
  loadingList.value = true;
  try {
    textbooks.value = await api.listTextbooks();
  } catch (e) {
    textbooks.value = [];
  } finally {
    loadingList.value = false;
  }
}

async function selectTextbook(t: TextbookInfo) {
  currentMeta.value = t;
  loadingContent.value = true;
  current.value = null;
  activeTocId.value = null;
  try {
    current.value = await api.readTextbook(t.id);
    await nextTick();
    // 默认激活第一个标题
    if (toc.value.length > 0) {
      activeTocId.value = toc.value[0].id;
    }
  } catch (e) {
    current.value = null;
  } finally {
    loadingContent.value = false;
  }
}

// ── 目录导航 ──
function scrollToHeading(item: TocItem) {
  activeTocId.value = item.id;
  const reader = document.querySelector(".textbook-reader");
  const el = document.getElementById(item.id);
  if (el && reader) {
    const readerRect = reader.getBoundingClientRect();
    const elRect = el.getBoundingClientRect();
    const offset = elRect.top - readerRect.top + reader.scrollTop - 24;
    reader.scrollTo({ top: offset, behavior: "smooth" });
  }
}

// 滚动时更新激活的目录项
function onReaderScroll() {
  const reader = document.querySelector(".textbook-reader");
  if (!reader || toc.value.length === 0) return;
  const scrollTop = (reader as HTMLElement).scrollTop;
  let currentId = toc.value[0].id;
  for (const item of toc.value) {
    const el = document.getElementById(item.id);
    if (!el) continue;
    const offsetTop = el.offsetTop;
    if (offsetTop - 80 <= scrollTop) {
      currentId = item.id;
    } else {
      break;
    }
  }
  activeTocId.value = currentId;
}

// ── 学科标签颜色 ──
function subjectVariant(subject: string): "default" | "math" | "english" | "politics" | "professional" {
  const map: Record<string, "default" | "math" | "english" | "politics" | "professional"> = {
    math: "math",
    english: "english",
    politics: "politics",
    professional: "professional",
    408: "professional",
  };
  return map[subject] ?? "default";
}

function subjectLabel(subject: string): string {
  const map: Record<string, string> = {
    math: "数学",
    english: "英语",
    politics: "政治",
    professional: "专业课",
    408: "专业课",
  };
  return map[subject] ?? subject;
}

/** 高亮搜索命中片段中的查询词 */
function highlightSnippet(snippet: string): string {
  const q = searchQuery.value.trim();
  if (!q) return escapeHtml(snippet);
  const escaped = escapeHtml(snippet);
  const qEscaped = escapeHtml(q);
  const re = new RegExp(`(${qEscaped.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")})`, "gi");
  return escaped.replace(re, '<mark class="hit-mark">$1</mark>');
}

onMounted(() => {
  loadTextbooks();
});
</script>

<template>
  <div class="textbooks-view">
    <!-- 左侧：教材列表 -->
    <aside class="list-panel" :class="{ collapsed: listCollapsed }">
      <!-- 顶部操作区 -->
      <div class="list-header">
        <Button variant="primary" size="sm" @click="openImportDialog" class="import-btn">
          <Plus :size="14" />
          <span class="import-text">导入教材</span>
        </Button>
        <button
          type="button"
          class="list-toggle"
          :title="listCollapsed ? '展开列表' : '折叠列表'"
          @click="toggleList"
        >
          <PanelLeftClose v-if="!listCollapsed" :size="16" />
          <PanelLeft v-else :size="16" />
        </button>
      </div>

      <!-- 搜索模式切换 + 输入 -->
      <div class="search-section">
        <div class="mode-switch">
          <button
            class="mode-btn"
            :class="{ active: searchMode === 'list' }"
            @click="switchMode('list')"
          >
            <Search :size="12" />
            按名称
          </button>
          <button
            class="mode-btn"
            :class="{ active: searchMode === 'content' }"
            @click="switchMode('content')"
          >
            <FileSearch :size="12" />
            搜内容
          </button>
        </div>
        <div class="search-box">
          <Search :size="14" class="search-icon" />
          <input
            v-model="searchQuery"
            type="text"
            :placeholder="searchMode === 'list' ? '筛选教材名称...' : '在所有教材中搜索...'"
            class="search-input"
            @input="onSearchInput"
          />
        </div>
      </div>

      <!-- 搜索结果（全文搜索模式） -->
      <div v-if="searchMode === 'content'" class="search-results">
        <LoadingSpinner v-if="searching" :size="20" label="搜索中..." />
        <div v-else-if="searchQuery.trim() && searchHits.length === 0" class="list-empty">
          <p>未在教材中找到 "{{ searchQuery }}"</p>
        </div>
        <div v-else-if="searchHits.length > 0" class="hit-list">
          <div class="hit-count">找到 {{ searchHits.length }} 处匹配</div>
          <button
            v-for="(hit, idx) in searchHits"
            :key="idx"
            class="hit-item"
            @click="jumpToSearchHit(hit)"
          >
            <div class="hit-head">
              <BookOpen :size="12" class="hit-icon" />
              <span class="hit-title">{{ hit.textbook_title }}</span>
              <Badge :variant="subjectVariant(hit.subject)" class="hit-subject-badge">
                {{ subjectLabel(hit.subject) }}
              </Badge>
              <span class="hit-line">L{{ hit.line_number }}</span>
            </div>
            <div class="hit-snippet" v-html="highlightSnippet(hit.snippet)"></div>
          </button>
        </div>
      </div>

      <!-- 教材列表（按名称筛选模式） -->
      <template v-else>
        <LoadingSpinner
          v-if="loadingList && textbooks.length === 0"
          :size="24"
          label="加载教材列表..."
        />

        <div v-else-if="groupedTextbooks.length === 0" class="list-empty">
          <p>{{ textbooks.length === 0 ? '暂无教材，点击「导入教材」添加' : '未找到匹配的教材' }}</p>
        </div>

        <nav v-else class="textbook-tree">
          <div
            v-for="group in groupedTextbooks"
            :key="group.subject"
            class="subject-group"
          >
            <div class="subject-header">
              <span class="subject-dot" :class="subjectVariant(group.subject)" />
              <span class="subject-name">{{ subjectLabel(group.subject) }}</span>
              <span class="subject-count">{{ group.items.length }}</span>
            </div>

            <div class="textbook-list">
              <button
                v-for="t in group.items"
                :key="t.id"
                class="textbook-item"
                :class="{ active: currentMeta?.id === t.id }"
                @click="selectTextbook(t)"
              >
                <BookOpen :size="14" class="tb-icon" />
                <div class="tb-meta">
                  <span class="tb-title">{{ t.title }}</span>
                  <span class="tb-filename text-mono">{{ t.filename }}</span>
                </div>
                <div class="tb-actions">
                  <span
                    class="tb-action-btn"
                    title="重命名教材"
                    @click="openRenameDialog(t, $event)"
                  >
                    <Pencil :size="13" />
                  </span>
                  <span
                    class="tb-action-btn"
                    title="删除教材"
                    @click="handleDelete(t, $event)"
                  >
                    <Trash2 :size="13" />
                  </span>
                </div>
              </button>
            </div>
          </div>
        </nav>
      </template>
    </aside>

    <!-- 右侧：阅读器 -->
    <section class="reader-panel">
      <div v-if="loadingContent" class="reader-empty">
        <LoadingSpinner :size="28" label="加载教材内容..." />
      </div>

      <div v-else-if="!current" class="reader-empty">
        <EmptyState
          title="选择一本教材"
          description="从左侧列表中选择教材开始阅读，或点击「导入教材」添加新教材。"
        >
          <template #actions>
            <div class="empty-hint">
              <BookOpen :size="20" />
              <span>共 {{ textbooks.length }} 本教材</span>
            </div>
          </template>
        </EmptyState>
      </div>

      <div v-else class="reader-layout">
        <!-- 阅读区 -->
        <div class="textbook-reader" @scroll="onReaderScroll">
          <div class="reader-header">
            <h1 class="reader-title">{{ currentMeta?.title ?? "教材" }}</h1>
            <div class="reader-badges">
              <Badge v-if="currentMeta" :variant="subjectVariant(currentMeta.subject)">
                {{ subjectLabel(currentMeta.subject) }}
              </Badge>
              <Badge v-if="currentMeta" variant="default">{{ currentMeta.filename }}</Badge>
            </div>
          </div>

          <div class="markdown-body" v-html="renderedHtml" />
        </div>

        <!-- 目录侧栏 -->
        <aside v-if="toc.length > 0" class="toc-panel">
          <div class="toc-head">
            <List :size="15" class="toc-icon" />
            <span>目录</span>
          </div>
          <nav class="toc-list">
            <button
              v-for="item in toc"
              :key="item.id"
              class="toc-item"
              :class="[`toc-l${item.level}`, { active: activeTocId === item.id }]"
              @click="scrollToHeading(item)"
            >
              <Hash v-if="item.level <= 2" :size="12" class="toc-hash" />
              <span class="toc-text">{{ item.text }}</span>
            </button>
          </nav>
        </aside>
      </div>
    </section>

    <!-- 导入教材对话框 -->
    <div v-if="showImportDialog" class="modal-overlay" @click.self="showImportDialog = false">
      <div class="modal-dialog">
        <div class="modal-header">
          <h3>导入教材</h3>
          <button class="modal-close" @click="showImportDialog = false">×</button>
        </div>
        <div class="modal-body">
          <div class="form-field">
            <label class="form-label">所属学科</label>
            <select v-model="importSubject" class="form-select">
              <option v-for="opt in subjectOptions" :key="opt.value" :value="opt.value">
                {{ opt.label }}
              </option>
            </select>
          </div>
          <div class="form-field">
            <label class="form-label">显示标题（可选）</label>
            <input
              v-model="importTitle"
              type="text"
              placeholder="留空则使用文件名"
              class="form-input"
            />
          </div>
          <p class="form-hint">
            点击「选择文件」后，将弹出系统文件选择器，支持 .md / .markdown / .txt 格式。
          </p>
        </div>
        <div class="modal-footer">
          <Button variant="ghost" size="sm" @click="showImportDialog = false">取消</Button>
          <Button variant="primary" size="sm" :loading="importing" @click="handleImport">
            选择文件并导入
          </Button>
        </div>
      </div>
    </div>

    <!-- 重命名教材对话框 -->
    <div v-if="showRenameDialog" class="modal-overlay" @click.self="closeRenameDialog">
      <div class="modal-dialog">
        <div class="modal-header">
          <h3>重命名教材</h3>
          <button class="modal-close" @click="closeRenameDialog">×</button>
        </div>
        <div class="modal-body">
          <div class="form-field">
            <label class="form-label">教材标题</label>
            <input
              ref="renameInput"
              v-model="renameNewTitle"
              type="text"
              placeholder="输入新的教材标题"
              class="form-input"
              @keydown.enter="handleRename"
            />
          </div>
          <p class="form-hint">标题不能包含 / \ : * ? " &lt; > | 等特殊字符。</p>
        </div>
        <div class="modal-footer">
          <Button variant="ghost" size="sm" @click="closeRenameDialog">取消</Button>
          <Button variant="primary" size="sm" :loading="renaming" @click="handleRename">
            确认重命名
          </Button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.textbooks-view {
  display: flex;
  height: 100%;
  min-height: 0;
}

/* ── 左侧列表 ── */
.list-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--divider-color);
  background: var(--bg-elevated);
}

.import-btn {
  flex: 1;
  justify-content: center;
}

.list-toggle {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  padding: 0;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--bg-primary);
  color: var(--text-secondary);
  cursor: pointer;
  transition: all var(--transition-fast);
  flex-shrink: 0;
}

.list-toggle:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
  border-color: var(--accent);
}

.search-section {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--divider-color);
  background: var(--bg-elevated);
}

.mode-switch {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 2px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
}

.mode-btn {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-1);
  padding: var(--space-1) var(--space-2);
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-family: inherit;
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: background-color var(--transition-fast), color var(--transition-fast);
}

.mode-btn:hover {
  color: var(--text-primary);
}

.mode-btn.active {
  background: var(--bg-elevated);
  color: var(--accent);
  font-weight: var(--font-semibold);
  box-shadow: var(--shadow-xs);
}

.list-panel {
  width: 280px;
  min-width: 280px;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--divider-color);
  background: var(--bg-elevated);
  overflow: hidden;
  transition: width 0.2s ease-out, min-width 0.2s ease-out;
  will-change: width;
}

.list-panel.collapsed {
  width: 44px;
  min-width: 44px;
}

.list-panel.collapsed .list-header {
  padding: var(--space-3) var(--space-2);
  justify-content: center;
  gap: 0;
}

.list-panel.collapsed .import-btn,
.list-panel.collapsed .search-section,
.list-panel.collapsed .search-results,
.list-panel.collapsed .textbook-tree,
.list-panel.collapsed .list-empty {
  display: none;
}

.search-box {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--divider-color);
}

.search-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.search-input {
  flex: 1;
  border: none;
  background: transparent;
  font-family: inherit;
  font-size: var(--text-sm);
  color: var(--text-primary);
  outline: none;
}

.search-input::placeholder {
  color: var(--text-tertiary);
}

.search-results {
  flex: 1;
  overflow-y: auto;
  padding: var(--space-2) var(--space-3) var(--space-4);
}

.hit-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.hit-count {
  padding: var(--space-2) var(--space-1);
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-weight: var(--font-medium);
}

.hit-item {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  padding: var(--space-3);
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: border-color var(--transition-fast), background var(--transition-fast);
  text-align: left;
  font-family: inherit;
}

.hit-item:hover {
  border-color: var(--accent);
  background: var(--accent-subtle);
}

.hit-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-width: 0;
}

.hit-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.hit-title {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}

.hit-subject-badge {
  flex-shrink: 0;
}

.hit-line {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-family: var(--font-mono);
  flex-shrink: 0;
}

.hit-snippet {
  font-size: var(--text-xs);
  color: var(--text-secondary);
  line-height: var(--leading-normal);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.hit-snippet :deep(mark) {
  background: var(--accent-subtle);
  color: var(--accent);
  border-radius: var(--radius-xs);
  padding: 0 2px;
  font-weight: var(--font-semibold);
}

.list-empty {
  padding: var(--space-8) var(--space-4);
  text-align: center;
  color: var(--text-tertiary);
  font-size: var(--text-sm);
}

.textbook-tree {
  flex: 1;
  overflow-y: auto;
  padding: var(--space-2) var(--space-2) var(--space-4);
}

.subject-group {
  margin-bottom: var(--space-3);
}

.subject-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-2);
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.subject-dot {
  width: 8px;
  height: 8px;
  border-radius: var(--radius-full);
  flex-shrink: 0;
}

.subject-dot.math { background: var(--color-math); }
.subject-dot.english { background: var(--color-english); }
.subject-dot.politics { background: var(--color-politics); }
.subject-dot.professional { background: var(--color-professional); }
.subject-dot.default { background: var(--text-quaternary); }

.subject-name {
  flex: 1;
}

.subject-count {
  font-size: var(--text-xs);
  color: var(--text-quaternary);
  font-weight: var(--font-medium);
}

.textbook-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.textbook-item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  width: 100%;
  padding: var(--space-2) var(--space-3);
  border: none;
  background: transparent;
  font-family: inherit;
  text-align: left;
  cursor: pointer;
  border-radius: var(--radius-sm);
  transition: all var(--transition-fast);
  color: var(--text-secondary);
}

.textbook-item:hover {
  background: var(--sidebar-item-hover);
  color: var(--text-primary);
}

.textbook-item.active {
  background: var(--accent-subtle);
  color: var(--accent);
}

.tb-icon {
  flex-shrink: 0;
  opacity: 0.8;
}

.tb-meta {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.tb-title {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tb-filename {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tb-actions {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
  opacity: 0;
  transition: opacity var(--transition-fast);
}

.textbook-item:hover .tb-actions,
.textbook-item.active .tb-actions {
  opacity: 1;
}

.tb-action-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: var(--radius-xs);
  color: var(--text-tertiary);
  cursor: pointer;
  transition: all var(--transition-fast);
}

.tb-action-btn:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.text-mono {
  font-family: var(--font-mono);
}

.tb-chevron {
  color: var(--text-quaternary);
  flex-shrink: 0;
}

/* ── 右侧阅读器 ── */
.reader-panel {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  display: flex;
}

.reader-empty {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--space-8);
  overflow: auto;
}

.reader-layout {
  flex: 1;
  display: flex;
  min-width: 0;
}

.textbook-reader {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
  padding: clamp(var(--space-6), 4vw, var(--space-10)) clamp(var(--space-6), 5vw, var(--space-12));
}

.reader-header {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  margin-bottom: var(--space-8);
  padding-bottom: var(--space-5);
  border-bottom: 1px solid var(--divider-color);
}

.reader-title {
  font-size: var(--text-2xl);
  font-weight: var(--font-bold);
  color: var(--text-primary);
  letter-spacing: -0.01em;
  line-height: var(--leading-tight);
  margin: 0;
}

.reader-badges {
  display: flex;
  gap: var(--space-2);
  flex-wrap: wrap;
}

/* ── Markdown 内容 ── */
.markdown-body {
  font-size: var(--text-base);
  line-height: var(--leading-relaxed);
  color: var(--text-primary);
  width: 100%;
  max-width: min(100%, 760px);
  margin: 0 auto;
}

.markdown-body :deep(.md-h) {
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  line-height: var(--leading-tight);
  margin-top: var(--space-8);
  margin-bottom: var(--space-3);
  scroll-margin-top: 80px;
}

.markdown-body :deep(.md-h1) {
  font-size: var(--text-2xl);
  padding-bottom: var(--space-2);
  border-bottom: 1px solid var(--divider-color);
}

.markdown-body :deep(.md-h2) {
  font-size: var(--text-xl);
}

.markdown-body :deep(.md-h3) {
  font-size: var(--text-lg);
}

.markdown-body :deep(.md-h4),
.markdown-body :deep(.md-h5),
.markdown-body :deep(.md-h6) {
  font-size: var(--text-base);
}

.markdown-body :deep(.md-p) {
  margin: 0 0 var(--space-4);
  color: var(--text-secondary);
}

.markdown-body :deep(.md-ul),
.markdown-body :deep(.md-ol) {
  margin: 0 0 var(--space-4);
  padding-left: var(--space-6);
  color: var(--text-secondary);
}

.markdown-body :deep(.md-ul) {
  list-style: disc;
}

.markdown-body :deep(.md-ol) {
  list-style: decimal;
}

.markdown-body :deep(.md-ul li),
.markdown-body :deep(.md-ol li) {
  margin-bottom: var(--space-2);
}

.markdown-body :deep(.md-code) {
  font-family: var(--font-mono);
  font-size: 0.88em;
  padding: 2px 6px;
  background: var(--bg-tertiary);
  border-radius: var(--radius-xs);
  color: var(--accent);
}

.markdown-body :deep(.md-pre) {
  margin: 0 0 var(--space-4);
  padding: var(--space-4);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
  overflow-x: auto;
  border: 1px solid var(--border-color);
}

.markdown-body :deep(.md-block-code) {
  font-family: var(--font-mono);
  font-size: var(--text-sm);
  line-height: var(--leading-relaxed);
  color: var(--text-primary);
  white-space: pre;
}

.markdown-body :deep(.md-link) {
  color: var(--accent);
  text-decoration: none;
  transition: color var(--transition-fast);
}

.markdown-body :deep(.md-link:hover) {
  color: var(--accent-hover);
  text-decoration: underline;
}

.markdown-body :deep(.md-quote) {
  margin: 0 0 var(--space-4);
  padding: var(--space-3) var(--space-4);
  border-left: 2px solid var(--divider-color);
  background: var(--bg-tertiary);
  border-radius: var(--radius-sm);
  color: var(--text-secondary);
}

.markdown-body :deep(.md-hr) {
  border: none;
  border-top: 1px solid var(--divider-color);
  margin: var(--space-6) 0;
}

.markdown-body :deep(strong) {
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

/* ── 目录侧栏 ── */
.toc-panel {
  width: 220px;
  min-width: 220px;
  border-left: 1px solid var(--divider-color);
  background: var(--bg-elevated);
  overflow-y: auto;
  padding: var(--space-4) var(--space-3);
}

.toc-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.04em;
  margin-bottom: var(--space-3);
  padding: 0 var(--space-1);
}

.toc-icon {
  color: var(--text-tertiary);
}

.toc-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.toc-item {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  width: 100%;
  padding: var(--space-1) var(--space-2);
  border: none;
  background: transparent;
  font-family: inherit;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  cursor: pointer;
  border-radius: var(--radius-xs);
  transition: all var(--transition-fast);
  text-align: left;
  line-height: var(--leading-normal);
}

.toc-item:hover {
  background: var(--sidebar-item-hover);
  color: var(--text-primary);
}

.toc-item.active {
  background: var(--accent-subtle);
  color: var(--accent);
  font-weight: var(--font-medium);
}

.toc-hash {
  flex-shrink: 0;
  opacity: 0.6;
}

.toc-text {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.toc-l3 { padding-left: var(--space-4); }
.toc-l4 { padding-left: var(--space-6); }
.toc-l5 { padding-left: var(--space-8); }
.toc-l6 { padding-left: var(--space-10); }

/* ── 导入教材对话框 ── */
.modal-overlay {
  position: fixed;
  inset: 0;
  z-index: 100;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-overlay);
  backdrop-filter: blur(6px);
}

.modal-dialog {
  width: 420px;
  max-width: calc(100vw - var(--space-8));
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-lg);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: var(--space-4) var(--space-5);
  border-bottom: 1px solid var(--divider-color);
}

.modal-header h3 {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  margin: 0;
}

.modal-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  font-size: 20px;
  line-height: 1;
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: background-color var(--transition-fast), color var(--transition-fast);
}

.modal-close:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.modal-body {
  padding: var(--space-5);
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.modal-footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-5);
  border-top: 1px solid var(--divider-color);
  background: var(--bg-primary);
}

.form-field {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.form-label {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
}

.form-input,
.form-select {
  height: 36px;
  padding: 0 var(--space-3);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--bg-primary);
  color: var(--text-primary);
  font-family: inherit;
  font-size: var(--text-sm);
  outline: none;
  transition: border-color var(--transition-fast), box-shadow var(--transition-fast);
}

.form-input:focus,
.form-select:focus {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-subtle);
}

.form-input::placeholder {
  color: var(--text-quaternary);
}

.form-select {
  cursor: pointer;
}

.form-hint {
  margin: 0;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  line-height: var(--leading-normal);
}

/* ── EmptyState 提示 ── */
.empty-hint {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--text-tertiary);
  font-size: var(--text-sm);
}

/* ── 响应式 ── */
@media (max-width: 1100px) {
  .toc-panel {
    display: none;
  }
}

@media (max-width: 900px) {
  .textbook-reader {
    padding: var(--space-5) var(--space-5) var(--space-10);
  }

  .markdown-body {
    max-width: 100%;
  }
}

@media (max-width: 720px) {
  .list-panel {
    width: 240px;
    min-width: 240px;
  }

  .list-panel.collapsed {
    width: 44px;
    min-width: 44px;
  }
}
</style>
