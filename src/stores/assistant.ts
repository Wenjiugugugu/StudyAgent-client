import { defineStore } from "pinia";
import { ref, computed } from "vue";
import * as api from "@/api";
import type { ChatMessage, AgentContext } from "@/types";

export const useAssistantStore = defineStore("assistant", () => {
  const messages = ref<ChatMessage[]>([]);
  const loading = ref(false);
  const panelOpen = ref(false);
  const context = ref<AgentContext>({});

  const visibleMessages = computed(() =>
    messages.value.filter((m) => m.role === "user" || m.role === "assistant")
  );

  function togglePanel() {
    panelOpen.value = !panelOpen.value;
  }

  function openPanel() {
    panelOpen.value = true;
  }

  function closePanel() {
    panelOpen.value = false;
  }

  function setContext(ctx: AgentContext) {
    context.value = ctx;
  }

  function clearMessages() {
    messages.value = [];
  }

  async function sendMessage(content: string) {
    messages.value.push({ role: "user", content });
    loading.value = true;
    try {
      const response = await api.chat({
        messages: [...messages.value],
        agent: "assistant",
        context: context.value,
      });
      messages.value.push({
        role: "assistant",
        content: response.content,
      });
    } catch (e) {
      messages.value.push({
        role: "assistant",
        content: `出错了：${e instanceof Error ? e.message : String(e)}`,
      });
    } finally {
      loading.value = false;
    }
  }

  return {
    messages,
    loading,
    panelOpen,
    context,
    visibleMessages,
    togglePanel,
    openPanel,
    closePanel,
    setContext,
    clearMessages,
    sendMessage,
  };
});
