<script setup lang="ts">
import Card from "@/components/ui/Card.vue";
import Button from "@/components/ui/Button.vue";
import Badge from "@/components/ui/Badge.vue";
import Select from "@/components/ui/Select.vue";
import Checkbox from "@/components/ui/Checkbox.vue";
import { useSettingsStore } from "@/stores/settings";
import { useProviderEditor } from "../composables/useProviderEditor";
import { ref } from "vue";
import {
  Bot,
  Plus,
  Trash2,
  Check,
  Eye,
  EyeOff,
  Zap,
  Pencil,
  RefreshCw,
  Search,
  Wallet,
  SlidersHorizontal,
} from "lucide-vue-next";

const settingsStore = useSettingsStore();

const {
  showProviderForm,
  editingProviderId,
  editingOriginalKey,
  showApiKey,
  testing,
  testResult,
  providerForm,
  modelList,
  modelListLoading,
  modelListError,
  showModelDropdown,
  modelSearchKeyword,
  filteredModels,
  modelContextLength,
  formatContextLength,
  providerTypeOptions,
  nameAutoFilled,
  markNameEdited,
  startAddProvider,
  editProvider,
  cancelProviderForm,
  saveProvider,
  removeProvider,
  setDefaultProvider,
  handleTestProvider,
  loadModelList,
  selectModel,
  balanceResults,
  balanceLoading,
  queryBalance,
  formatBalance,
  supportsBalance,
} = useProviderEditor();

// ── 功能 → Provider 分配（核心 3 类） ──
interface FeatureSpec {
  key: string;
  label: string;
  desc: string;
  /** 时效性要求高：建议选择支持联网搜索或知识库较新的 Provider */
  timeSensitive: boolean;
}

const FEATURES: FeatureSpec[] = [
  {
    key: "assistant",
    label: "进度表 / 考纲生成",
    desc: "AI 依据最新考纲生成进度表节点",
    timeSensitive: true,
  },
  {
    key: "planner",
    label: "周计划与目标倒排",
    desc: "生成每周每日任务排程与目标倒排估时",
    timeSensitive: false,
  },
  {
    key: "reviewer",
    label: "复盘分析",
    desc: "整理复盘数据并给出后续建议",
    timeSensitive: false,
  },
];

const featureSaving = ref(false);
const featureSavedFlash = ref(false);
/** 功能 Provider 分配保存失败信息（保存失败时回滚本地选择并提示） */
const featureSaveError = ref("");
let featureSaveTimer: number | null = null;

async function onFeatureProviderChange(feature: string, providerId: string | number | null) {
  const value = providerId == null ? "" : String(providerId);
  const previous = settingsStore.settings?.feature_providers?.[feature] ?? "";
  settingsStore.setFeatureProvider(feature, value);
  if (!settingsStore.settings) return;
  featureSaveError.value = "";
  featureSaving.value = true;
  try {
    await settingsStore.save();
    featureSavedFlash.value = true;
    if (featureSaveTimer != null) window.clearTimeout(featureSaveTimer);
    featureSaveTimer = window.setTimeout(() => {
      featureSavedFlash.value = false;
    }, 1500);
  } catch (e) {
    // 保存失败：回滚下拉选择，避免界面显示已生效但实际未落盘
    settingsStore.setFeatureProvider(feature, previous);
    featureSaveError.value = `保存失败：${e instanceof Error ? e.message : String(e)}`;
  } finally {
    featureSaving.value = false;
  }
}
</script>

<template>
  <!-- AI Provider 配置区 -->
  <Card id="settings-ai-provider" padding="lg" class="settings-section">
    <div class="section-head">
      <div class="section-title">
        <Bot :size="18" />
        <span>AI Provider</span>
      </div>
      <Button variant="secondary" size="sm" @click="startAddProvider">
        <Plus :size="14" />
        <span>添加</span>
      </Button>
    </div>

    <!-- Provider 列表 -->
    <div class="item-list">
      <div
        v-for="provider in settingsStore.aiProviders"
        :key="provider.id"
        class="item-row"
      >
        <div class="item-info">
          <div class="item-name-row">
            <span class="item-name">{{ provider.name }}</span>
            <Badge v-if="provider.is_default" variant="success">默认</Badge>
            <Badge v-if="!provider.enabled" variant="default">已禁用</Badge>
          </div>
          <div class="item-sub">
            <span>{{ provider.type }}</span>
            <span v-if="provider.model">· {{ provider.model }}</span>
          </div>
          <div class="item-sub text-mono">{{ provider.base_url }}</div>
          <!-- 余额查询结果（cc-Switch 风格：行内展示剩余/已用额度） -->
          <div
            v-if="balanceResults[provider.id]"
            class="item-sub balance-line"
            :class="{ 'balance-error': !balanceResults[provider.id].success }"
          >
            {{
              balanceResults[provider.id].success
                ? `余额 ${formatBalance(balanceResults[provider.id])}`
                : `余额查询失败：${balanceResults[provider.id].message}`
            }}
          </div>
        </div>
        <div class="item-actions">
          <Button
            v-if="!provider.is_default"
            variant="ghost"
            size="sm"
            @click="setDefaultProvider(provider.id)"
          >
            设为默认
          </Button>
          <!-- 无余额 API 的供应商（Gemini/Anthropic/Ollama/通义/火山/LongCat/MiMo 等）不显示 -->
          <Button
            v-if="supportsBalance(provider)"
            variant="ghost"
            size="sm"
            icon
            :disabled="balanceLoading[provider.id]"
            :aria-label="`查询余额 ${provider.name}`"
            :title="balanceLoading[provider.id] ? '查询中…' : '查询余额'"
            @click="queryBalance(provider)"
          >
            <RefreshCw v-if="balanceLoading[provider.id]" :size="14" class="spin" />
            <Wallet v-else :size="14" />
          </Button>
          <Button variant="ghost" size="sm" icon :aria-label="`编辑 ${provider.name}`" @click="editProvider(provider)">
            <Pencil :size="14" />
          </Button>
          <Button variant="ghost" size="sm" icon :aria-label="`删除 ${provider.name}`" @click="removeProvider(provider.id)">
            <Trash2 :size="14" />
          </Button>
        </div>
      </div>

      <div v-if="settingsStore.aiProviders.length === 0" class="empty-inline">
        尚未配置 AI Provider，点击「添加」开始。
      </div>
    </div>

    <!-- Provider 编辑表单 -->
    <div v-if="showProviderForm" class="edit-form">
      <div class="form-title">
        {{ editingProviderId ? "编辑 Provider" : "新增 Provider" }}
      </div>
      <div class="form-grid">
        <div class="form-field">
          <label class="form-label">名称</label>
          <input
            v-model="providerForm.name"
            type="text"
            class="form-input"
            :placeholder="nameAutoFilled ? '' : '我的 Provider'"
            @input="markNameEdited"
          />
        </div>
        <div class="form-field">
          <label class="form-label">模型提供商</label>
          <Select v-model="providerForm.type" class="select-autowidth">
            <option v-for="opt in providerTypeOptions" :key="opt.value" :value="opt.value">
              {{ opt.label }}
            </option>
          </Select>
        </div>
        <div class="form-field form-field-full">
          <label class="form-label">Base URL</label>
          <input v-model="providerForm.base_url" type="text" class="form-input" placeholder="https://api.openai.com/v1" />
        </div>
        <div class="form-field form-field-full">
          <label class="form-label">API Key</label>
          <div class="input-with-action">
            <input
              v-model="providerForm.api_key"
              :type="showApiKey ? 'text' : 'password'"
              class="form-input"
              :placeholder="editingOriginalKey ? '已配置（留空保持不变）' : 'sk-...'"
            />
            <button class="input-suffix-btn" type="button" @click="showApiKey = !showApiKey">
              <component :is="showApiKey ? EyeOff : Eye" :size="15" />
            </button>
          </div>
        </div>
        <div class="form-field">
          <label class="form-label">Model</label>
          <div class="model-selector">
            <div class="model-input-row">
              <input
                v-model="providerForm.model"
                type="text"
                class="form-input"
                placeholder="gpt-4o"
                @focus="showModelDropdown = modelList.length > 0"
              />
              <button
                type="button"
                class="model-fetch-btn"
                :disabled="modelListLoading"
                :title="modelListLoading ? '加载中…' : '获取模型列表'"
                @click="loadModelList"
              >
                <RefreshCw v-if="modelListLoading" :size="14" class="spin" />
                <Search v-else :size="14" />
              </button>
            </div>
            <p v-if="modelListError" class="model-error">{{ modelListError }}</p>
            <div v-if="showModelDropdown && filteredModels.length > 0" class="model-dropdown">
              <div class="model-search">
                <Search :size="13" />
                <input
                  v-model="modelSearchKeyword"
                  type="text"
                  placeholder="搜索模型…"
                  class="model-search-input"
                />
              </div>
              <div class="model-list">
                <button
                  v-for="m in filteredModels"
                  :key="m.id"
                  type="button"
                  class="model-item"
                  :class="{ active: m.id === providerForm.model }"
                  @click="selectModel(m.id)"
                >
                  <span class="model-id">{{ m.id }}</span>
                  <span v-if="modelContextLength(m)" class="model-ctx">
                    {{ formatContextLength(modelContextLength(m)) }} ctx
                  </span>
                </button>
              </div>
            </div>
          </div>
        </div>
        <div class="form-field">
          <label class="form-label">Temperature</label>
          <input v-model.number="providerForm.temperature" type="number" step="0.1" min="0" max="2" class="form-input" />
        </div>
        <div class="form-field">
          <label class="form-label">Max Tokens</label>
          <input v-model.number="providerForm.max_tokens" type="number" min="1" class="form-input" />
        </div>
        <div class="form-field form-field-checkbox">
          <label class="checkbox-label">
            <Checkbox v-model="providerForm.is_default" />
            <span>设为默认 Provider</span>
          </label>
        </div>
      </div>

      <div v-if="testResult" class="test-result" :class="{ error: testResult.includes('失败') || testResult.includes('错误') }">
        <Zap :size="14" />
        <span>{{ testResult }}</span>
      </div>

      <div class="form-actions">
        <Button variant="secondary" size="sm" :loading="testing" @click="handleTestProvider">
          <Zap :size="14" />
          <span>测试连接</span>
        </Button>
        <div class="form-actions-right">
          <Button variant="ghost" size="sm" @click="cancelProviderForm">取消</Button>
          <Button variant="primary" size="sm" :loading="testing" @click="saveProvider">
            <Check :size="14" />
            <span>{{ testing ? '测试中…' : '保存' }}</span>
          </Button>
        </div>
      </div>
    </div>

    <!-- 功能 → Provider 分配（与 Provider 配置同一张卡片） -->
    <div id="settings-feature-providers" class="feature-block">
      <div class="feature-block-head">
        <div class="section-title">
          <SlidersHorizontal :size="16" />
          <span>功能 Provider 分配</span>
        </div>
        <span v-if="featureSavedFlash" class="saved-flash"><Check :size="13" /> 已保存</span>
        <span v-else-if="featureSaving" class="saved-flash">保存中…</span>
      </div>
      <p class="section-desc">
        为不同功能单独指定 AI Provider。未指定的功能使用默认 Provider。
      </p>
      <p v-if="featureSaveError" class="feature-save-error">{{ featureSaveError }}</p>

      <div v-if="settingsStore.aiProviders.length === 0" class="empty-inline">
        请先在上方添加 AI Provider。
      </div>

      <div v-else class="feature-map">
        <div
          v-for="f in FEATURES"
          :key="f.key"
          class="feature-row"
        >
          <div class="feature-info">
            <div class="feature-name-row">
              <span class="feature-name">{{ f.label }}</span>
              <Badge v-if="f.timeSensitive" variant="warning">时效敏感</Badge>
            </div>
            <div class="feature-sub">{{ f.desc }}</div>
            <div v-if="f.timeSensitive" class="feature-hint">
              建议选择自带联网搜索能力或知识库更新较新的 API（云端大模型更合适），避免知识滞后导致考纲与教材内容过时。本地模型（Ollama）的知识取决于加载的权重，<strong>不建议</strong>用于此功能。
            </div>
          </div>
          <Select
            class="feature-select"
            :model-value="settingsStore.settings?.feature_providers?.[f.key] ?? ''"
            @update:model-value="(v) => onFeatureProviderChange(f.key, v)"
          >
            <option value="">跟随默认 Provider</option>
            <option v-for="p in settingsStore.aiProviders" :key="p.id" :value="p.id">
              {{ p.name }} · {{ p.model || p.type }}
            </option>
          </Select>
        </div>
      </div>
    </div>
  </Card>
</template>

<style scoped>
/* 功能 Provider 分配：与 Provider 配置同一张卡片，用分隔线区分子区块 */
.feature-block {
  margin-top: var(--space-5);
  padding-top: var(--space-4);
  border-top: 1px solid var(--border-color);
}
.feature-block-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-bottom: var(--space-2);
}
.feature-block-head .section-title {
  font-size: var(--text-sm);
}
.section-desc {
  margin: 0 0 var(--space-3) 0;
  font-size: var(--text-sm);
  color: var(--text-tertiary);
  line-height: 1.5;
}
.feature-save-error {
  margin: 0 0 var(--space-3) 0;
  font-size: var(--text-sm);
  color: var(--color-danger);
  line-height: 1.5;
}
.saved-flash {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: var(--text-xs);
  color: var(--color-success, #34c759);
}
.feature-map {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}
.feature-row {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  flex-wrap: wrap;
}
.feature-info {
  flex: 1;
  min-width: 240px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.feature-name-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-wrap: wrap;
}
.feature-name {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}
.feature-sub {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}
.feature-hint {
  margin-top: 4px;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  line-height: 1.5;
}
.feature-select {
  min-width: 220px;
}
.empty-inline {
  padding: var(--space-3);
  font-size: var(--text-sm);
  color: var(--text-tertiary);
  text-align: center;
}
</style>
