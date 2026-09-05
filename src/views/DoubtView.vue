<script setup lang="ts">
/**
 * 解惑 — AI 引导式答疑页
 *
 * 设计能力：
 * 1. AI 答疑（后端 Doubt Agent，苏格拉底式引导：先提问引导定位考点、分步推进，
 *    用户明示不会或多次卡住时给出完整讲解）
 * 2. 调取本地已导入教材：发送题目时自动全文检索，命中片段注入为「本地教材参考」
 * 3. 联网：Doubt system prompt 已授权模型联网查证（取决于 Provider 是否支持）
 * 4. 查询具体题目：粘贴题目后按引导流程逐轮推进，支持多轮上下文
 * 5. 特定回答方式：引导式（默认）与直接讲解可切换
 */
import { ref, computed, onMounted, nextTick } from "vue";
import * as api from "@/api";
import { useSettingsStore } from "@/stores/settings";
import { useAiRequest } from "@/composables/useAiRequest";
import { renderMessage } from "@/composables/useRenderLatex";
import Select from "@/components/ui/Select.vue";
import {
  HelpCircle,
  Send,
  Trash2,
  Loader2,
  Square,
  BookOpen,
  FileSearch,
  Sparkles,
} from "lucide-vue-next";
import type { ChatMessage, TextbookInfo, TextbookSearchHit } from "@/types";

type DoubtMessage = { role: "user" | "assistant"; content: string };

/** 回答方式：引导式（默认）/ 直接讲解 */
const mode = ref<"guide" | "direct">("guide");

const settingsStore = useSettingsStore();

const textbooks = ref<TextbookInfo[]>([]);
const selectedTextbookId = ref<string>("");
const textbooksLoading = ref(false);

const messages = ref<DoubtMessage[]>([]);
const inputText = ref("");
const textbookHits = ref<TextbookSearchHit[]>([]);
const hitLoading = ref(false);

const ai = useAiRequest();
const loading = ai.pending;
const aiError = ai.error;

const messagesContainer = ref<HTMLElement | null>(null);
const textareaRef = ref<HTMLTextAreaElement | null>(null);

const suggestions = [
  "帮我分析这道题的考点",
  "这道题我卡住了，引导我一下",
  "讲一下这个知识点的解题思路",
];

const providerInfo = computed(() => {
  // 优先默认 provider；未设置默认时兜底显示第一个已配置的 provider
  const p = settingsStore.defaultProvider ?? settingsStore.aiProviders[0];
  if (p) {
    return `${p.name} · ${p.model}`;
  }
  return "未配置 AI Provider";
});

const hitNote = computed(() =>
  textbookHits.value.length > 0
    ? `已在 ${textbookHits.value.length} 处教材内容中检索到相关段落`
    : null
);

/** ── 不同回答方式的提示文案（随模式切换实时变化） ── */
const modeCopy = computed(() =>
  mode.value === "guide"
    ? {
        subtitle: "苏格拉底式引导提问，帮你自己想通问题",
        emptyDesc:
          "引导式模式下，AI 会先提问帮你定位考点、分步推进，你自己想通后再继续；明确表示不会或多次卡住时才给出完整讲解。",
        placeholder: "把题目或疑问粘贴到这里，我会先提问引导你自己想通…",
        hint: "回车发送，Shift+Enter 换行。想直接看答案时回复「讲答案吧」即可。",
        welcome:
          "你好，我是你的考研解惑导师，当前为「引导式」答疑。\n\n我会先用提问引导你自己想通题目（先定位考点、分步推进），而不是直接给答案；当你明确表示「讲答案吧」或多次卡住时，我会给出完整讲解。\n\n如果已导入教材，我会自动检索教材内容作为参考。",
      }
    : {
        subtitle: "直接给出完整、规范的题目讲解",
        emptyDesc:
          "直接讲解模式下，AI 收到题目后会直接给出完整、规范的解题过程（含思路、步骤与结论），不进行分步引导。",
        placeholder: "把题目或疑问粘贴到这里，我会直接给出完整讲解…",
        hint: "回车发送，Shift+Enter 换行。直接讲解模式下 AI 将一次性给出完整解答。",
        welcome:
          "你好，我是你的考研解惑导师，当前为「直接讲解」答疑。\n\n你把题目（或疑问）发给我后，我会直接给出完整、规范的解题过程（含思路、步骤与结论），不做分步引导。\n\n如果已导入教材，我会自动检索教材内容作为参考。",
      }
);

/** 欢迎消息：每次进入页面展示当前模式的引导规则 */
function welcomeMessage(): DoubtMessage {
  return {
    role: "assistant",
    content: modeCopy.value.welcome,
  };
}

/** 切换回答方式：同步更新欢迎消息（对话刚开始时） */
function setMode(next: "guide" | "direct") {
  if (mode.value === next) return;
  mode.value = next;
  ai.clearError();
  // 对话里还没有用户提问时，刷新欢迎消息展示新模式说明
  const hasUserMsg = messages.value.some((m) => m.role === "user");
  if (!hasUserMsg) {
    messages.value = [welcomeMessage()];
    scrollToBottom();
  }
}

onMounted(async () => {
  messages.value = [welcomeMessage()];
  await loadTextbooks();
});

async function loadTextbooks() {
  textbooksLoading.value = true;
  try {
    textbooks.value = await api.listTextbooks();
  } catch {
    textbooks.value = [];
  } finally {
    textbooksLoading.value = false;
  }
}

function scrollToBottom() {
  nextTick(() => {
    if (messagesContainer.value) {
      messagesContainer.value.scrollTop = messagesContainer.value.scrollHeight;
    }
  });
}

function autoResize() {
  const el = textareaRef.value;
  if (!el) return;
  el.style.height = "auto";
  el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
}

/**
 * 在本地教材中检索题目相关段落，命中片段注入上下文。
 * 选中教材时优先该校命中；未选时全库检索。
 */
async function collectTextbookSnippets(text: string): Promise<string> {
  hitLoading.value = true;
  textbookHits.value = [];
  try {
    const hits = await api.searchInTextbook(text);
    const selected = selectedTextbookId.value;
    const ordered = selected
      ? [...hits.filter((h) => h.textbook_id === selected), ...hits.filter((h) => h.textbook_id !== selected)]
      : hits;
    const top = ordered.slice(0, 3);
    textbookHits.value = top;
    if (top.length === 0) return "";
    const blocks = top.map(
      (h) => `《${h.textbook_title}》 第 ${h.line_number} 行附近：\n> ${h.snippet.trim()}`
    );
    return `【本地教材参考】\n${blocks.join("\n\n")}`;
  } catch {
    return "";
  } finally {
    hitLoading.value = false;
  }
}

function handleInput() {
  autoResize();
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    handleSend();
  }
}

function useSuggestion(text: string) {
  inputText.value = text;
  autoResize();
  handleSend();
}

function clearConversation() {
  messages.value = [welcomeMessage()];
  textbookHits.value = [];
  ai.clearError();
}

async function cancel() {
  try {
    await api.cancelAiRequest(api.AI_CANCEL_KEYS.doubt);
  } catch {
    // 取消失败不阻塞，下次请求自然恢复
  }
}

async function handleSend() {
  const text = inputText.value.trim();
  if (!text || loading.value) return;

  messages.value.push({ role: "user", content: text });
  inputText.value = "";
  nextTick(autoResize);
  scrollToBottom();

  // 组装 user 消息：题目 + 教材参考片段 + 回答方式指令
  let fullContent = text;
  const textbookRef = await collectTextbookSnippets(text);
  if (textbookRef) fullContent += `\n\n${textbookRef}`;
  if (mode.value === "direct") {
    fullContent += `\n\n（本次请直接给出完整、规范的讲解，无需分步引导。）`;
  }

  const history: ChatMessage[] = messages.value.slice(0, -1).map((m) => ({
    role: m.role,
    content: m.content,
  }));

  try {
    const response = await ai.run(
      () =>
        api.chatDoubt({
          messages: [...history, { role: "user", content: fullContent }],
          agent: "doubt",
          context: {
            current_view: "doubt",
            additional: {
              answer_mode: mode.value,
              textbook_snippets: textbookHits.value.map((h) => ({
                textbook_title: h.textbook_title,
                line_number: h.line_number,
                snippet: h.snippet,
              })),
            },
          },
        }),
      "解惑请求失败"
    );
    if (response) {
      messages.value.push({ role: "assistant", content: response.content });
    }
  } catch {
    // 错误已由 useAiRequest 记录到 aiError，统一在消息区渲染
  } finally {
    scrollToBottom();
  }
}
</script>

<template>
  <div class="doubt-view">
    <div class="doubt-container">
    <!-- Header -->
    <header class="doubt-header">
      <div class="header-info">
        <div class="header-icon">
          <HelpCircle :size="20" :stroke-width="1.5" />
        </div>
        <div class="header-text">
          <div class="header-title-row">
            <h1 class="header-title">解惑</h1>
            <span class="header-badge">测试版</span>
          </div>
          <transition name="mode-copy" mode="out-in">
              <p :key="mode" class="header-subtitle">{{ modeCopy.subtitle }}</p>
            </transition>
        </div>
      </div>
      <div class="header-actions">
        <!-- 回答方式切换 -->
        <div class="mode-switch">
          <span class="mode-thumb" :class="{ right: mode === 'direct' }" aria-hidden="true" />
          <button
            type="button"
            class="mode-chip"
            :class="{ active: mode === 'guide' }"
            @click="setMode('guide')"
          >
            <Sparkles :size="13" :stroke-width="1.5" />
            <span>引导式</span>
          </button>
          <button
            type="button"
            class="mode-chip"
            :class="{ active: mode === 'direct' }"
            @click="setMode('direct')"
          >
            <HelpCircle :size="13" :stroke-width="1.5" />
            <span>直接讲解</span>
          </button>
        </div>
        <button
          class="icon-btn"
          title="清空对话"
          :disabled="messages.length <= 1 || loading"
          @click="clearConversation"
        >
          <Trash2 :size="16" :stroke-width="1.5" />
        </button>
      </div>
    </header>

    <!-- 教材上下文栏 -->
    <div class="context-bar">
      <div class="context-item">
        <BookOpen :size="15" :stroke-width="1.5" class="context-icon" />
        <label class="context-label" for="textbook-select">参考教材</label>
        <Select
          id="textbook-select"
          v-model="selectedTextbookId"
          :disabled="textbooksLoading"
          class="select-autowidth"
        >
          <option value="">
            {{ textbooks.length > 0 ? "全部已导入教材（自动检索）" : "未导入教材（不检索，直接作答）" }}
          </option>
          <option v-for="t in textbooks" :key="t.id" :value="t.id">{{ t.title }}</option>
        </Select>
      </div>
      <div class="context-note">
        <FileSearch :size="14" :stroke-width="1.5" />
        <span>{{ hitNote || (textbooks.length > 0 ? "发送题目时自动在教材中检索相关段落" : "未导入教材时可直接解答，前往「教材」页可导入") }}</span>
      </div>
    </div>

    <!-- Messages -->
    <div ref="messagesContainer" class="messages-container">
      <!-- Empty State -->
      <div v-if="messages.length === 0" class="empty-state">
        <div class="empty-icon">
          <HelpCircle :size="48" :stroke-width="1.2" />
        </div>
        <h2 class="empty-title">AI 解惑导师</h2>
        <p class="empty-desc">
          {{ modeCopy.emptyDesc }}
        </p>
        <div class="suggestions">
          <button
            v-for="s in suggestions"
            :key="s"
            class="suggestion-chip"
            @click="useSuggestion(s)"
          >
            {{ s }}
          </button>
        </div>
      </div>

      <!-- Message List -->
      <template v-else>
        <div
          v-for="(msg, i) in messages"
          :key="i"
          class="message"
          :class="msg.role"
        >
          <div class="message-bubble">
            <div
              v-if="msg.role === 'assistant'"
              class="message-content latex-content"
              v-html="renderMessage(msg.content)"
            />
            <p v-else class="message-content">{{ msg.content }}</p>
          </div>
        </div>

        <!-- 教材检索中 -->
        <div v-if="hitLoading" class="message assistant">
          <div class="message-bubble loading-bubble">
            <FileSearch :size="14" class="loading-icon" />
            <span>正在教材中检索相关段落…</span>
          </div>
        </div>

        <!-- 请求失败横幅 -->
        <div v-if="aiError" class="error-banner">
          <span>{{ aiError }}</span>
        </div>

        <!-- AI 思考中 -->
        <div v-if="loading" class="message assistant">
          <div class="message-bubble loading-bubble">
            <Loader2 :size="14" class="loading-icon" />
            <span>正在引导你思考…</span>
          </div>
        </div>
      </template>
    </div>

    <!-- Input Area -->
    <div class="input-area">
      <div class="input-wrapper">
        <textarea
          ref="textareaRef"
          v-model="inputText"
          class="input-field"
          :placeholder="modeCopy.placeholder"
          rows="1"
          :disabled="loading"
          @keydown="handleKeydown"
          @input="handleInput"
        />
        <button
          v-if="loading"
          class="stop-btn"
          title="停止"
          @click="cancel"
        >
          <Square :size="15" :stroke-width="1.5" />
        </button>
        <button
          v-else
          class="send-btn"
          :disabled="!inputText.trim()"
          @click="handleSend"
        >
          <Send :size="16" :stroke-width="1.5" />
        </button>
      </div>
      <div class="input-foot">
        <span class="input-hint">{{ modeCopy.hint }}</span>
        <span class="provider-info">{{ providerInfo }}</span>
      </div>
    </div>
    </div>
  </div>
</template>

<style scoped>
.doubt-view {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: var(--space-5) var(--space-6);
  overflow: hidden;
  box-sizing: border-box;
}

/* 内部内容容器：最大宽度 + 居中，四周留白（类似 DebugView 布局） */
.doubt-container {
  display: flex;
  flex-direction: column;
  width: 100%;
  max-width: 960px;
  margin: 0 auto;
  height: 100%;
  min-height: 0;
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
  overflow: hidden;
}

/* ── Header ── */
.doubt-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-3) var(--space-6);
  min-height: 60px;
  border-bottom: 1px solid var(--divider-color);
  background: var(--bg-secondary);
  gap: var(--space-3);
  flex-wrap: wrap;
}

.header-info {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.header-icon {
  width: 36px;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--accent-subtle);
  color: var(--accent);
  border-radius: var(--radius-md);
  flex-shrink: 0;
}

.header-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.header-title {
  font-size: var(--text-lg);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  margin: 0;
  line-height: var(--leading-tight);
}

.header-title-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

/* 「测试版」徽标：标题右侧小号胶囊标记 */
.header-badge {
  flex-shrink: 0;
  font-size: 10px;
  line-height: 1;
  font-weight: var(--font-medium);
  color: var(--accent);
  background: var(--accent-subtle);
  border: 1px solid var(--accent-soft, var(--border-color));
  border-radius: 999px;
  padding: 3px 7px;
  letter-spacing: 0.02em;
}

.header-subtitle {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  margin: 0;
}

/* 模式副标题切换过渡 */
.mode-copy-enter-active,
.mode-copy-leave-active {
  transition: opacity 0.22s ease, transform 0.22s ease;
}
.mode-copy-enter-from {
  opacity: 0;
  transform: translateY(4px);
}
.mode-copy-leave-to {
  opacity: 0;
  transform: translateY(-2px);
}

.header-actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

/* 回答方式切换：分段控件 + 滑动指示块 */
.mode-switch {
  position: relative;
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 2px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-full);
  background: var(--bg-tertiary);
}

.mode-thumb {
  position: absolute;
  top: 2px;
  bottom: 2px;
  left: 2px;
  width: calc(50% - 2px);
  border-radius: var(--radius-full);
  background: var(--accent);
  pointer-events: none;
  z-index: 0;
  transition: transform 0.28s cubic-bezier(0.32, 0.72, 0, 1);
}

.mode-thumb.right {
  transform: translateX(100%);
}

.mode-chip {
  position: relative;
  z-index: 1;
  flex: 1;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-1);
  padding: var(--space-1) var(--space-3);
  border: none;
  border-radius: var(--radius-full);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  font-family: inherit;
  cursor: pointer;
  transition: color var(--transition-fast);
  white-space: nowrap;
}

.mode-chip:hover {
  color: var(--text-primary);
}

.mode-chip.active {
  color: var(--text-on-accent);
}

.icon-btn {
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: all var(--transition-fast);
}

.icon-btn:hover:not(:disabled) {
  background: var(--sidebar-item-hover);
  color: var(--text-primary);
}

.icon-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

/* ── 教材上下文栏 ── */
.context-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-6);
  border-bottom: 1px solid var(--divider-color);
  background: var(--bg-secondary);
  flex-wrap: wrap;
}

.context-item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.context-icon {
  color: var(--accent);
  flex-shrink: 0;
}

.context-label {
  font-size: var(--text-xs);
  color: var(--text-secondary);
  font-weight: var(--font-medium);
}

.context-note {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  font-size: 11px;
  color: var(--text-tertiary);
}

/* ── Messages ── */
.messages-container {
  flex: 1;
  overflow-y: auto;
  padding: var(--space-5) var(--space-6);
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

/* ── Empty State ── */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
  padding: var(--space-10) var(--space-6);
  max-width: 520px;
  margin: 0 auto;
}

.empty-icon {
  color: var(--accent);
  margin-bottom: var(--space-4);
}

.empty-title {
  font-size: var(--text-xl);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  margin: 0 0 var(--space-3);
}

.empty-desc {
  font-size: var(--text-sm);
  color: var(--text-secondary);
  line-height: var(--leading-relaxed);
  margin: 0 0 var(--space-6);
}

.suggestions {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  width: 100%;
}

.suggestion-chip {
  padding: var(--space-2) var(--space-4);
  border: 1px solid var(--border-color);
  background: var(--bg-elevated);
  color: var(--text-secondary);
  font-size: var(--text-sm);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: all var(--transition-fast);
  text-align: left;
}

.suggestion-chip:hover {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-subtle);
}

/* ── Message Bubbles ── */
.message {
  display: flex;
  max-width: 100%;
}

.message.user {
  justify-content: flex-end;
}

.message.assistant {
  justify-content: flex-start;
}

.message-bubble {
  max-width: 78%;
  padding: var(--space-3) var(--space-4);
  border-radius: var(--radius-lg);
  font-size: var(--text-sm);
  line-height: var(--leading-relaxed);
}

.message.user .message-bubble {
  background: var(--accent);
  color: var(--text-on-accent);
  font-weight: var(--font-medium);
  border-bottom-right-radius: var(--radius-xs);
}

.message.assistant .message-bubble {
  background: var(--bg-tertiary);
  color: var(--text-primary);
  border-bottom-left-radius: var(--radius-xs);
}

.loading-bubble {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--text-tertiary);
}

.loading-icon {
  animation: spin 1s linear infinite;
}

.message-content {
  white-space: pre-wrap;
  word-break: break-word;
}

/* 错误横幅 */
.error-banner {
  align-self: flex-start;
  max-width: 78%;
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--color-danger, #ef4444);
  border-radius: var(--radius-md);
  background: rgba(239, 68, 68, 0.08);
  color: var(--color-danger, #ef4444);
  font-size: var(--text-xs);
}

/* ── LaTeX Rendering ── */
.latex-content {
  line-height: 1.8;
}

.latex-content :deep(.latex-block) {
  display: flex;
  justify-content: center;
  margin: var(--space-3) 0;
  overflow-x: auto;
}

.latex-content :deep(.latex-inline) {
  display: inline;
}

.latex-content :deep(.katex) {
  font-size: 1.05em;
}

.latex-content :deep(.latex-block .katex) {
  font-size: 1.1em;
}

/* ── Input Area ── */
.input-area {
  padding: var(--space-3) var(--space-6) var(--space-4);
  border-top: 1px solid var(--divider-color);
  background: var(--bg-secondary);
}

.input-wrapper {
  display: flex;
  align-items: flex-end;
  gap: var(--space-2);
  background: var(--bg-tertiary);
  border-radius: var(--radius-lg);
  padding: var(--space-1) var(--space-1) var(--space-1) var(--space-4);
}

.input-field {
  flex: 1;
  border: none;
  background: transparent;
  color: var(--text-primary);
  font-size: var(--text-sm);
  font-family: inherit;
  resize: none;
  outline: none;
  padding: var(--space-2) 0;
  max-height: 160px;
  line-height: var(--leading-normal);
}

.input-field::placeholder {
  color: var(--text-quaternary);
}

.input-field:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.send-btn,
.stop-btn {
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: var(--accent);
  color: var(--text-on-accent);
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: all var(--transition-fast);
  flex-shrink: 0;
}

.send-btn:hover:not(:disabled) {
  background: var(--accent-hover);
}

.send-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.stop-btn {
  background: var(--color-danger, #ef4444);
}

.stop-btn:hover {
  opacity: 0.9;
}

.input-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  margin-top: var(--space-2);
  flex-wrap: wrap;
}

.input-hint {
  font-size: 11px;
  color: var(--text-quaternary);
}

.provider-info {
  font-size: 11px;
  color: var(--text-tertiary);
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
