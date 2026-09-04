<script setup lang="ts">
/**
 * 独立「进度」页：按科目管理各科进度表，并支持「考纲方案」（variant）切换。
 *
 * 每个科目有多个考纲方案（如数学：数一/数二/数三；专业课：408/307/法硕…），
 * 一次仅一个方案被启用（active_variant），未启用的方案仅展示为折叠的标签。
 * 切换方案会调用后端 set_active_progress_variant，启用表会同步对齐到该方案。
 */
import { ref, computed, onMounted } from "vue";
import * as api from "@/api";
import Button from "@/components/ui/Button.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import ProgressTableView from "@/components/progress/ProgressTableView.vue";
import BatchProgressModal from "@/components/progress/BatchProgressModal.vue";
import { ChevronRight, ChevronDown, FolderOpen, ListChecks } from "lucide-vue-next";
import type { ProgressIndex } from "@/types";

const SUBJECTS: { key: string; label: string; desc: string }[] = [
  { key: "math", label: "数学", desc: "数一 / 数二 / 数三" },
  { key: "english", label: "英语", desc: "英一 / 英二" },
  { key: "politics", label: "政治", desc: "政治" },
  { key: "professional", label: "专业课", desc: "408 / 307 / 法硕 / 311…" },
];

const loading = ref(true);
const error = ref("");
const index = ref<ProgressIndex | null>(null);
/** 设置中考试类型解析出的「科目 → 默认方案」（如 数二 → math/数二） */
const settingsVariants = ref<Record<string, string>>({});

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

async function reload() {
  loading.value = true;
  error.value = "";
  try {
    const [idx, variants] = await Promise.all([
      api.listProgressTables(),
      api.defaultProgressVariants(),
    ]);
    index.value = idx;
    settingsVariants.value = variants;
  } catch (e) {
    error.value = `加载进度失败：${errMsg(e)}`;
  } finally {
    loading.value = false;
  }
}

/** 某科目当前启用方案（优先级：已启用方案 > 设置考试类型推断 > 该科首个方案） */
function activeVariantOf(subject: string): string {
  const variants = api.PROGRESS_VARIANTS[subject] ?? [];
  const active = index.value?.subjects[subject]?.active_variant;
  if (active && variants.includes(active)) return active;
  const fromSettings = settingsVariants.value[subject];
  if (fromSettings && variants.includes(fromSettings)) return fromSettings;
  return variants[0] ?? "默认";
}

/** 设置考试类型涉及的科目（未设置考试类型时为空数组 = 不做科目门控，全部自动同步） */
const settingsSubjects = computed(() => Object.keys(settingsVariants.value));

async function pickVariant(subject: string, variant: string) {
  if (variant === activeVariantOf(subject)) return;
  try {
    await api.setActiveProgressVariant(subject, variant);
    await reload();
  } catch (e) {
    error.value = `切换方案失败：${errMsg(e)}`;
  }
}

// ── 专业课方案折叠：只醒目显示当前启用，其余专业课全部折叠到一处 ──
const profVariants = api.PROGRESS_VARIANTS.professional ?? [];
const showMoreProfessional = ref(false);
const activeProf = computed(() => activeVariantOf("professional"));
const otherProfessional = computed(() => profVariants.filter((v) => v !== activeProf.value));

function pickProfessional(variant: string) {
  showMoreProfessional.value = false;
  pickVariant("professional", variant);
}

// ── 批量更改进度（标题右侧入口） ──
const showBatchModal = ref(false);
/** 弹窗内各科当前启用方案 */
const batchVariants = computed<Record<string, string>>(() =>
  Object.fromEntries(SUBJECTS.map((s) => [s.key, activeVariantOf(s.key)]))
);
/** 应用批量进度后刷新数据，保持弹窗打开展示结果 */
async function onBatchApplied() {
  await reload();
}

onMounted(reload);
</script>

<template>
  <div class="progress-page">
    <header class="page-head">
      <h1 class="page-title">进度</h1>
      <Button
        variant="secondary"
        size="sm"
        class="batch-btn"
        title="快速批量调整各科每本书的进度（选择学到第几章）"
        @click="showBatchModal = true"
      >
        <ListChecks :size="14" /> 批量更改进度
      </Button>
    </header>
    <div v-if="error" class="error-banner" role="alert">{{ error }}</div>

    <LoadingSpinner v-if="loading" :size="30" label="加载进度..." class="page-loading" />

    <div v-else class="subjects">
      <section v-for="s in SUBJECTS" :key="s.key" class="subject-card">
        <div class="subject-head">
          <FolderOpen :size="16" class="subject-icon" />
          <h2 class="subject-name">{{ s.label }}</h2>
          <span class="subject-desc">{{ s.desc }}</span>
        </div>

        <!-- 方案选择条：单选启用，未启用折叠为标签；专业课其余方案折叠到一处 -->
        <div class="variant-bar">
          <template v-if="s.key === 'professional'">
            <button type="button" class="variant-chip active" title="当前启用专业课">
              <FolderOpen :size="12" /> {{ activeProf }}
            </button>
            <button
              v-if="otherProfessional.length"
              type="button"
              class="more-btn"
              @click="showMoreProfessional = !showMoreProfessional"
            >
              <component
                :is="showMoreProfessional ? ChevronDown : ChevronRight"
                :size="12"
              />
              {{ showMoreProfessional ? "收起" : "其他专业课" }}
              <span class="more-count">{{ otherProfessional.length }}</span>
            </button>
          </template>
          <template v-else>
            <button
              v-for="v in api.PROGRESS_VARIANTS[s.key] ?? []"
              :key="v"
              type="button"
              class="variant-chip"
              :class="{ active: v === activeVariantOf(s.key) }"
              @click="pickVariant(s.key, v)"
            >
              <span class="chip-label">{{ v }}</span>
              <ChevronRight v-if="v !== activeVariantOf(s.key)" :size="12" class="chip-folded" />
            </button>
          </template>
        </div>

        <!-- 专业课折叠区：其余专业课全部折叠到这里，点击即切换启用 -->
        <div v-if="s.key === 'professional' && showMoreProfessional" class="prof-more">
          <button
            v-for="v in otherProfessional"
            :key="v"
            type="button"
            class="variant-chip"
            @click="pickProfessional(v)"
          >
            {{ v }}
          </button>
        </div>

        <!-- 启用方案的进度表编辑器（不随方案重挂载，避免切方案时跳回顶部；组件内部监听 variant 静默刷新） -->
        <div class="variant-panel">
          <ProgressTableView
            :key="s.key"
            :subject="s.key"
            :variant="activeVariantOf(s.key)"
            :enabled-subjects="settingsSubjects"
          />
        </div>
      </section>
    </div>

    <!-- 批量更改进度：选择每本书「学到第几章」，总表按教材自动推导 -->
    <BatchProgressModal
      :open="showBatchModal"
      :index="index"
      :variants="batchVariants"
      @close="showBatchModal = false"
      @applied="onBatchApplied"
    />
  </div>
</template>

<style scoped>
.progress-page {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  padding: var(--space-5) var(--space-6);
  gap: var(--space-5);
  overflow-y: auto;
}
.page-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  flex-shrink: 0;
}
.page-title {
  margin: 0;
  font-size: var(--text-2xl);
  font-weight: var(--font-bold);
  color: var(--text-primary);
  letter-spacing: -0.02em;
}
.batch-btn {
  flex-shrink: 0;
}
.page-sub {
  margin: 0;
  font-size: var(--text-sm);
  color: var(--text-tertiary);
}
.error-banner {
  margin-top: var(--space-2);
  padding: 8px 12px;
  border-radius: 8px;
  background: var(--color-danger-subtle);
  border: 1px solid var(--color-danger);
  color: var(--color-danger);
  font-size: 12px;
}
.page-loading {
  margin: auto;
}
.subjects {
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
}
.subject-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-4);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-lg);
  background: var(--bg-elevated);
}
.subject-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.subject-icon { color: var(--accent); }
.subject-name {
  margin: 0;
  font-size: var(--text-lg);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}
.subject-desc {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  margin-left: auto;
}
.variant-bar {
  display: flex;
  gap: var(--space-2);
  flex-wrap: wrap;
}
.variant-chip {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  padding: 5px 12px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-full);
  background: var(--bg-primary);
  color: var(--text-secondary);
  font-family: inherit;
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  cursor: pointer;
  transition: all var(--transition-fast);
}
.variant-chip:hover { border-color: var(--border-color-strong); color: var(--text-primary); }
.variant-chip.active {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-subtle);
}
.chip-folded { opacity: 0.6; }
.more-btn {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  padding: 5px 12px;
  border: 1px dashed var(--border-color-strong);
  border-radius: var(--radius-full);
  background: var(--bg-tertiary);
  color: var(--text-tertiary);
  font-family: inherit;
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  cursor: pointer;
  transition: all var(--transition-fast);
}
.more-btn:hover { color: var(--text-secondary); border-color: var(--border-color-strong); }
.more-count {
  font: var(--weight-medium) var(--text-caption) var(--font-mono);
  color: var(--text-tertiary);
}
.prof-more {
  display: flex;
  gap: var(--space-2);
  flex-wrap: wrap;
  padding: var(--space-3);
  border: 1px dashed var(--border-color);
  border-radius: var(--radius-md);
  background: var(--bg-tertiary);
}
.variant-panel {
  min-height: 0;
  border-top: 1px solid var(--divider-color);
  padding-top: var(--space-3);
}
.variant-panel :deep(.progress-view) {
  height: 420px;
  min-height: 420px;
}
</style>