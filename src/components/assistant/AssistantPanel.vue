<script setup lang="ts">
import { ref, nextTick, watch, computed } from "vue";
import { useRoute } from "vue-router";
import { useAssistantStore } from "@/stores/assistant";
import { useSettingsStore } from "@/stores/settings";
import { Sparkles, Send, X, Trash2, Loader2 } from "lucide-vue-next";
import { renderMessage } from "@/composables/useRenderLatex";

const route = useRoute();
const assistantStore = useAssistantStore();
const settingsStore = useSettingsStore();

const inputText = ref("");
const messagesContainer = ref<HTMLElement | null>(null);

const currentView = computed(() => route.meta.title as string);

function scrollToBottom() {
  nextTick(() => {
    if (messagesContainer.value) {
      messagesContainer.value.scrollTop = messagesContainer.value.scrollHeight;
    }
  });
}

watch(() => assistantStore.messages.length, scrollToBottom);

async function handleSend() {
  const text = inputText.value.trim();
  if (!text || assistantStore.loading) return;
  inputText.value = "";
  assistantStore.setContext({ current_view: route.name as string });
  await assistantStore.sendMessage(text);
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    handleSend();
  }
}

const suggestions = computed(() => {
  const view = route.name as string;
  const map: Record<string, string[]> = {
    dashboard: ["本周学习进度如何？", "哪些任务有风险？", "帮我分析学习趋势"],
    today: ["这个任务的重点是什么？", "帮我规划今天的时间", "解释一下这个知识点"],
    "week-plan": ["生成本周计划", "调整本周工作量", "分析本周目标"],
    knowledge: ["解释这个知识点", "这个知识点的依赖关系", "相关真题有哪些"],
    review: ["帮我写复盘总结", "分析今日困难原因", "给出改进建议"],
  };
  return map[view] ?? ["有什么学习问题？", "帮我制定学习计划", "讲解一个知识点"];
});
</script>

<template>
  <aside class="assistant-panel">
    <!-- Panel Header -->
    <div class="panel-header">
      <div class="header-info">
        <div class="header-icon">
          <Sparkles :size="16" :stroke-width="1.5" />
        </div>
        <div class="header-text">
          <span class="header-title">AI 助手</span>
          <span class="header-subtitle">上下文：{{ currentView }}</span>
        </div>
      </div>
      <div class="header-actions">
        <button class="icon-btn" title="清空对话" @click="assistantStore.clearMessages()">
          <Trash2 :size="15" :stroke-width="1.5" />
        </button>
        <button class="icon-btn" title="关闭" @click="assistantStore.closePanel()">
          <X :size="16" :stroke-width="1.5" />
        </button>
      </div>
    </div>

    <!-- Messages -->
    <div ref="messagesContainer" class="messages-container">
      <!-- Empty State -->
      <div v-if="assistantStore.visibleMessages.length === 0" class="empty-chat">
        <div class="empty-chat-icon">
          <Sparkles :size="32" :stroke-width="1.5" />
        </div>
        <p class="empty-chat-title">AI 学习助手</p>
        <p class="empty-chat-desc">基于当前页面上下文，为你答疑解惑</p>
        <div class="suggestions">
          <button
            v-for="s in suggestions"
            :key="s"
            class="suggestion-chip"
            @click="inputText = s; handleSend()"
          >
            {{ s }}
          </button>
        </div>
      </div>

      <!-- Message List -->
      <template v-else>
        <div
          v-for="(msg, i) in assistantStore.visibleMessages"
          :key="i"
          class="message"
          :class="msg.role"
        >
          <div class="message-bubble">
            <div v-if="msg.role === 'assistant'" class="message-content latex-content" v-html="renderMessage(msg.content)" />
            <p v-else class="message-content">{{ msg.content }}</p>
          </div>
        </div>

        <!-- Loading -->
        <div v-if="assistantStore.loading" class="message assistant">
          <div class="message-bubble loading-bubble">
            <Loader2 :size="14" class="loading-icon" />
            <span>思考中...</span>
          </div>
        </div>
      </template>
    </div>

    <!-- Input -->
    <div class="input-area">
      <div class="input-wrapper">
        <textarea
          v-model="inputText"
          class="input-field"
          placeholder="输入消息..."
          rows="1"
          @keydown="handleKeydown"
        />
        <button
          class="send-btn"
          :disabled="!inputText.trim() || assistantStore.loading"
          @click="handleSend"
        >
          <Send :size="16" :stroke-width="1.5" />
        </button>
      </div>
      <div class="provider-info">
        <span v-if="settingsStore.defaultProvider">
          {{ settingsStore.defaultProvider.name }} · {{ settingsStore.defaultProvider.model }}
        </span>
        <span v-else class="text-tertiary">未配置 AI Provider</span>
      </div>
    </div>
  </aside>
</template>

<style scoped>
.assistant-panel {
  width: var(--assistant-width);
  min-width: var(--assistant-width);
  height: 100vh;
  background: var(--bg-secondary);
  border-left: 1px solid var(--divider-color);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-3) var(--space-4);
  height: 56px;
  min-height: 56px;
  border-bottom: 1px solid var(--divider-color);
}

.header-info {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.header-icon {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--accent-subtle);
  color: var(--accent);
  border-radius: var(--radius-sm);
}

.header-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.header-title {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.header-subtitle {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.header-actions {
  display: flex;
  gap: var(--space-1);
}

.icon-btn {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  border-radius: var(--radius-xs);
  cursor: pointer;
  transition: all var(--transition-fast);
}

.icon-btn:hover {
  background: var(--sidebar-item-hover);
  color: var(--text-primary);
}

.messages-container {
  flex: 1;
  overflow-y: auto;
  padding: var(--space-4);
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.empty-chat {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
  padding: var(--space-8) var(--space-4);
}

.empty-chat-icon {
  color: var(--text-quaternary);
  margin-bottom: var(--space-3);
}

.empty-chat-title {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.empty-chat-desc {
  font-size: var(--text-sm);
  color: var(--text-tertiary);
  margin-top: var(--space-1);
  margin-bottom: var(--space-6);
}

.suggestions {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  width: 100%;
}

.suggestion-chip {
  padding: var(--space-2) var(--space-3);
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
  max-width: 85%;
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-lg);
  font-size: var(--text-sm);
  line-height: var(--leading-relaxed);
}

.message.user .message-bubble {
  background: var(--accent);
  color: #ffffff;
  font-weight: var(--font-medium);
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.15);
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

.input-area {
  padding: var(--space-3) var(--space-4);
  border-top: 1px solid var(--divider-color);
}

.input-wrapper {
  display: flex;
  align-items: flex-end;
  gap: var(--space-2);
  background: var(--bg-tertiary);
  border-radius: var(--radius-lg);
  padding: var(--space-1) var(--space-1) var(--space-1) var(--space-3);
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
  max-height: 120px;
  line-height: var(--leading-normal);
}

.send-btn {
  width: 28px;
  height: 28px;
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

.provider-info {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  margin-top: var(--space-2);
  text-align: center;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}
</style>
