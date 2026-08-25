<script setup lang="ts">
import Card from "@/components/ui/Card.vue";
import Button from "@/components/ui/Button.vue";
import Badge from "@/components/ui/Badge.vue";
import Select from "@/components/ui/Select.vue";
import { useSettingsStore } from "@/stores/settings";
import { useProviderEditor } from "../composables/useProviderEditor";
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
  startAddProvider,
  editProvider,
  cancelProviderForm,
  saveProvider,
  removeProvider,
  setDefaultProvider,
  handleTestProvider,
  loadModelList,
  selectModel,
} = useProviderEditor();
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
          <input v-model="providerForm.name" type="text" class="form-input" placeholder="我的 Provider" />
        </div>
        <div class="form-field">
          <label class="form-label">类型</label>
          <Select v-model="providerForm.type" :max-width="'220px'">
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
            <input v-model="providerForm.is_default" type="checkbox" class="form-checkbox" />
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
  </Card>
</template>
