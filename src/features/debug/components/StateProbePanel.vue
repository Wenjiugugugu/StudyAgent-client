<script setup lang="ts">
/**
 * 调试页 — State 解析测试面板
 */
import { ref } from "vue";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import { Boxes, RefreshCw } from "lucide-vue-next";
import { debugApi } from "../api";
import { formatJson } from "../utils/json";
import { statusBadge, statusLabel } from "../utils/status";
import type { TestResult } from "../types";
import type { StudyState } from "@/types";

const stateTest = ref<TestResult<StudyState>>({ status: "idle", data: null, error: null });

async function runStateTest() {
  stateTest.value = { status: "loading", data: null, error: null };
  try {
    const data = await debugApi.getState();
    stateTest.value = { status: "success", data, error: null };
  } catch (e) {
    stateTest.value = {
      status: "error",
      data: null,
      error: e instanceof Error ? e.message : String(e),
    };
  }
}

defineExpose({ refresh: runStateTest });
</script>

<template>
  <Card id="debug-state" padding="lg" class="debug-section">
    <div class="section-head">
      <div class="section-title">
        <Boxes :size="18" />
        <span>State 解析测试</span>
      </div>
      <div class="section-actions">
        <Badge :variant="statusBadge(stateTest.status)" size="sm">
          {{ statusLabel(stateTest.status) }}
        </Badge>
        <Button variant="ghost" size="sm" @click="runStateTest">
          <RefreshCw :size="14" />
          <span>测试</span>
        </Button>
      </div>
    </div>

    <div v-if="stateTest.error" class="error-text">{{ stateTest.error }}</div>
    <LoadingSpinner v-if="stateTest.status === 'loading'" :size="20" label="调用 api.getState()..." />
    <pre v-if="stateTest.data" class="code-block">{{ formatJson(stateTest.data) }}</pre>
    <div v-if="stateTest.status === 'idle'" class="empty-inline">点击「测试」调用 api.getState()。</div>
  </Card>
</template>
