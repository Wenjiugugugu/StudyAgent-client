<script setup lang="ts">
/**
 * 滴答清单同步 — 设置区块
 *
 * - 启用开关：写 settings.ticktick.enabled（独立保存，立即生效）
 * - Token：存入系统凭据库（keyring），不落 settings.json 明文；留空则使用环境变量 DIDA_TOKEN
 * - 立即同步：对今天执行一次按日对账，展示新建/更新/删除计数
 */
import { ref, computed, onMounted } from "vue";
import Card from "@/components/ui/Card.vue";
import Button from "@/components/ui/Button.vue";
import Badge from "@/components/ui/Badge.vue";
import { useSettingsStore } from "@/stores/settings";
import { settingsApi } from "../api";
import type { DidaProject } from "../api";
import { Cloud, RefreshCw, KeyRound, Check } from "lucide-vue-next";

const settingsStore = useSettingsStore();

const enabled = computed(() => settingsStore.settings?.ticktick?.enabled ?? false);

const token = ref("");
const hasToken = ref(false);
const tokenSaving = ref(false);
const tokenSavedFlash = ref(false);
const tokenError = ref<string | null>(null);
const toggleSaving = ref(false);
const toggleSavedFlash = ref(false);
const syncRunning = ref(false);
const syncResult = ref<string | null>(null);

// 归属清单选择（M5）：任务写入的滴答清单，留空 = 自动选择「学习」/首个未关闭清单
const projects = ref<DidaProject[]>([]);
const projectsLoading = ref(false);
const projectSaving = ref(false);
const selectedProjectId = ref("");
const currentProjectId = computed(() => settingsStore.settings?.ticktick?.project_id ?? "");

/** Token 状态徽标（随保存结果实时切换） */
const tokenBadge = computed(() => {
  if (tokenSavedFlash.value) return { variant: "success" as const, text: "已保存" };
  if (tokenError.value) return { variant: "danger" as const, text: "保存失败" };
  if (tokenSaving.value) return { variant: "info" as const, text: "保存中…" };
  if (hasToken.value) return { variant: "success" as const, text: "已配置" };
  return { variant: "default" as const, text: "未配置" };
});

async function refreshTokenStatus() {
  try {
    hasToken.value = await settingsApi.getDidaTokenStatus();
  } catch {
    hasToken.value = false;
  }
}

/** 拉取滴答清单列表（仅在已启用同步时拉取；失败静默为空） */
async function loadProjects() {
  if (!enabled.value) {
    projects.value = [];
    return;
  }
  projectsLoading.value = true;
  try {
    projects.value = await settingsApi.listDidaProjects();
  } catch {
    projects.value = [];
  } finally {
    projectsLoading.value = false;
  }
}

/** 保存归属清单选择（独立保存，立即生效） */
async function changeProject() {
  if (!settingsStore.settings) return;
  settingsStore.settings.ticktick = {
    ...(settingsStore.settings.ticktick ?? { enabled: false, tag_prefix: "计划" }),
    project_id: selectedProjectId.value,
  };
  projectSaving.value = true;
  try {
    await settingsStore.save();
  } catch {
    // 保存失败回滚显示为当前已持久化值
    selectedProjectId.value = currentProjectId.value;
  } finally {
    projectSaving.value = false;
  }
}

onMounted(() => {
  void refreshTokenStatus();
  selectedProjectId.value = currentProjectId.value;
  void loadProjects();
});

async function toggleEnabled() {
  if (!settingsStore.settings) return;
  settingsStore.settings.ticktick = {
    ...(settingsStore.settings.ticktick ?? {}),
    enabled: !enabled.value,
  };
  toggleSaving.value = true;
  try {
    await settingsStore.save();
    toggleSavedFlash.value = true;
    setTimeout(() => (toggleSavedFlash.value = false), 1800);
    if (enabled.value) void loadProjects();
  } catch (e) {
    // 保存失败时回滚开关
    if (settingsStore.settings) {
      settingsStore.settings.ticktick = {
        ...(settingsStore.settings.ticktick ?? {}),
        enabled: !enabled.value,
      };
    }
  } finally {
    toggleSaving.value = false;
  }
}

async function saveToken() {
  const value = token.value.trim();
  if (!value || tokenSaving.value) return;
  tokenSaving.value = true;
  tokenError.value = null;
  try {
    await settingsApi.setDidaToken(value);
    token.value = "";
    await refreshTokenStatus();
    if (enabled.value) void loadProjects();
    tokenSavedFlash.value = true;
    setTimeout(() => (tokenSavedFlash.value = false), 1800);
  } catch (e) {
    tokenError.value = e instanceof Error ? e.message : String(e);
  } finally {
    tokenSaving.value = false;
  }
}

async function runSync() {
  syncRunning.value = true;
  syncResult.value = null;
  try {
    syncResult.value = await settingsApi.syncDidaNow();
  } catch (e) {
    syncResult.value = e instanceof Error ? e.message : String(e);
  } finally {
    syncRunning.value = false;
  }
}
</script>

<template>
  <Card id="settings-dida-sync" padding="lg" class="settings-section">
    <div class="section-head">
      <div class="section-title">
        <Cloud :size="18" />
        <span>滴答清单同步</span>
      </div>
      <Badge :variant="enabled ? 'success' : 'default'">
        {{ enabled ? "已启用" : "未启用" }}
      </Badge>
    </div>

    <p class="section-desc">
      同步每日任务到滴答清单，手机端查看并勾选；只读写本系统创建的任务。
    </p>

    <!-- 启用开关（独立保存，瞬时生效） -->
    <div class="toggle-row">
      <div class="toggle-info">
        <span class="toggle-title">启用同步</span>
        <span class="toggle-desc">生成/重排日计划时自动对账到滴答</span>
      </div>
      <Button
        variant="secondary"
        size="sm"
        :loading="toggleSaving"
        @click="toggleEnabled"
      >
        <Check v-if="toggleSavedFlash" :size="14" />
        <span>{{ toggleSavedFlash ? "已保存" : enabled ? "禁用" : "启用" }}</span>
      </Button>
    </div>

    <!-- Token 配置 -->
    <div class="form-grid">
      <div class="form-field form-field-full">
        <label class="form-label token-label">
          <span class="label-left">
            <KeyRound :size="13" class="label-icon" />
            滴答 Token（API 口令）
          </span>
          <Badge :variant="tokenBadge.variant">{{ tokenBadge.text }}</Badge>
        </label>
        <div class="model-input-row">
          <input
            v-model="token"
            type="password"
            class="form-input"
            :class="{ 'form-input-ok': hasToken && !token }"
            :placeholder="hasToken ? '已保存（输入可替换）' : '网页版滴答清单 → 设置 → 账户与安全 → API 口令'"
            autocomplete="off"
          />
          <Button variant="primary" size="sm" :loading="tokenSaving" :disabled="!token.trim()" @click="saveToken">
            <Check :size="14" />
            <span>{{ tokenSavedFlash ? "已保存" : "保存" }}</span>
          </Button>
        </div>
        <span v-if="tokenError" class="item-sub field-hint token-error">{{ tokenError }}</span>
      </div>
    </div>

    <!-- 归属清单（M5）：选择任务写入的滴答清单 -->
    <div class="item-row">
      <div class="item-info">
        <span class="item-name">归属清单</span>
        <span class="item-sub">
          {{ projectsLoading ? "加载清单中…" : "任务写入的滴答清单；留空则自动选「学习」或首个未关闭清单" }}
        </span>
      </div>
      <select
        v-model="selectedProjectId"
        class="form-input project-select"
        autocomplete="off"
        :disabled="!enabled || projectsLoading || projectSaving"
        @change="changeProject"
      >
        <option value="">自动选择</option>
        <option v-for="p in projects" :key="p.id" :value="p.id">{{ p.name }}</option>
      </select>
    </div>

    <!-- 立即同步 -->
    <div class="item-row">
      <div class="item-info">
        <span class="item-name">立即同步今天</span>
        <span class="item-sub" v-if="syncResult">{{ syncResult }}</span>
        <span class="item-sub" v-else>手动触发一次今天的按日对账（需先启用同步并配置 Token）</span>
      </div>
      <Button
        variant="secondary"
        size="sm"
        :loading="syncRunning"
        :disabled="!enabled"
        @click="runSync"
      >
        <RefreshCw :size="14" />
        <span>同步</span>
      </Button>
    </div>
  </Card>
</template>

<style scoped>
/* Token 标签行：左侧图标+文字，右侧状态徽标 */
.token-label {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
}

.label-left {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
}

/* 已配置时输入框的轻提示态（配合 placeholder「已保存」） */
.form-input-ok {
  border-color: color-mix(in srgb, var(--color-success) 40%, transparent);
}

.token-error {
  color: var(--color-danger);
}

/* 归属清单选择：与输入框同宽，右侧对齐 */
.project-select {
  width: min(240px, 50%);
  cursor: pointer;
}
.project-select:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}
</style>