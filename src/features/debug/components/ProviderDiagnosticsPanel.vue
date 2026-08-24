<script setup lang="ts">
/**
 * 调试页 — AI Provider 诊断面板
 */
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import { Bot, RefreshCw, Zap } from "lucide-vue-next";
import { useProviderDiagnostics } from "../composables/useProviderDiagnostics";
import { statusBadge, statusLabel } from "../utils/status";

const { providerTests, loadProviders, testProvider, testAllProviders } = useProviderDiagnostics();

defineExpose({ refresh: loadProviders });
</script>

<template>
  <Card id="debug-providers" padding="lg" class="debug-section">
    <div class="section-head">
      <div class="section-title">
        <Bot :size="18" />
        <span>AI Provider 测试</span>
      </div>
      <Button
        v-if="providerTests.length > 0"
        variant="ghost"
        size="sm"
        @click="testAllProviders"
      >
        <RefreshCw :size="14" />
        <span>全部测试</span>
      </Button>
    </div>

    <div v-if="providerTests.length === 0" class="empty-inline">
      尚未配置 AI Provider。
    </div>

    <div v-else class="provider-list">
      <div v-for="(item, idx) in providerTests" :key="item.provider.id" class="provider-row">
        <div class="provider-info">
          <span class="provider-name">{{ item.provider.name }}</span>
          <span class="provider-sub text-mono">{{ item.provider.type }} · {{ item.provider.model }}</span>
        </div>
        <div class="provider-actions">
          <Badge :variant="statusBadge(item.status)" size="sm">
            {{ statusLabel(item.status) }}
          </Badge>
          <Button variant="secondary" size="sm" :loading="item.status === 'loading'" @click="testProvider(idx)">
            <Zap :size="14" />
            <span>测试</span>
          </Button>
        </div>
        <div v-if="item.message" class="provider-message" :class="{ error: item.status === 'error' }">
          {{ item.message }}
        </div>
      </div>
    </div>
  </Card>
</template>
