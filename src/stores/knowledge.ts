import { defineStore } from "pinia";
import { ref } from "vue";
import * as api from "@/api";
import type { KnowledgeSubjectIndex, KnowledgeObject, KnowledgeGraph } from "@/types";

export const useKnowledgeStore = defineStore("knowledge", () => {
  const index = ref<KnowledgeSubjectIndex[]>([]);
  const current = ref<KnowledgeObject | null>(null);
  const graph = ref<KnowledgeGraph | null>(null);
  const searchResults = ref<KnowledgeObject[]>([]);
  const loading = ref(false);
  const searchQuery = ref("");

  async function loadIndex(subject?: string) {
    loading.value = true;
    try {
      index.value = await api.listKnowledge(subject);
    } finally {
      loading.value = false;
    }
  }

  async function loadKnowledge(id: string) {
    loading.value = true;
    try {
      current.value = await api.getKnowledge(id);
    } finally {
      loading.value = false;
    }
  }

  async function loadGraph(subject: string) {
    loading.value = true;
    try {
      graph.value = await api.getKnowledgeGraph(subject);
    } finally {
      loading.value = false;
    }
  }

  async function search(query: string) {
    searchQuery.value = query;
    if (!query.trim()) {
      searchResults.value = [];
      return;
    }
    loading.value = true;
    try {
      searchResults.value = await api.searchKnowledge(query);
    } finally {
      loading.value = false;
    }
  }

  return {
    index,
    current,
    graph,
    searchResults,
    loading,
    searchQuery,
    loadIndex,
    loadKnowledge,
    loadGraph,
    search,
  };
});
