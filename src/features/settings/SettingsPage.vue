<script setup lang="ts">
/**
 * 设置页 — 主容器
 *
 * 由原 SettingsView.vue 拆分而来：负责装配各 Section 子组件、左侧导航、
 * 悬浮保存按钮与「未保存修改」离开确认弹窗，并在挂载时加载设置/学习状态。
 * 共享样式见 settings-base.css。
 */
import { onMounted, watch } from "vue";
import { useRoute } from "vue-router";
import { useSettingsStore } from "@/stores/settings";
import { useUpdateStore } from "@/stores/update";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import Button from "@/components/ui/Button.vue";
import Modal from "@/components/ui/Modal.vue";
import { Check, Save, AlertCircle } from "lucide-vue-next";
import { useSettingsForm } from "./composables/useSettingsForm";
import { useSectionNavigation } from "./composables/useSectionNavigation";
import PersonalInfoSection from "./sections/PersonalInfoSection.vue";
import GeneralSection from "./sections/GeneralSection.vue";
import AppearanceSection from "./sections/AppearanceSection.vue";
import GoalsSection from "./sections/GoalsSection.vue";
import ScheduleSection from "./sections/ScheduleSection.vue";
import RhythmSection from "./sections/RhythmSection.vue";
import TimeAllocationSection from "./sections/TimeAllocationSection.vue";
import TextbooksSection from "./sections/TextbooksSection.vue";
import AIProviderSection from "./sections/AIProviderSection.vue";
import DidaSyncSection from "./sections/DidaSyncSection.vue";
import McpSection from "./sections/McpSection.vue";
import StorageSection from "./sections/StorageSection.vue";
import UpdateSection from "./sections/UpdateSection.vue";
import "./settings-base.css";

const settingsStore = useSettingsStore();
const updateStore = useUpdateStore();
const route = useRoute();

// ── 主表单状态与保存逻辑 ──
const {
  weekdayOptions,
  form,
  textbookForm,
  studyState,
  stateLoading,
  stateError,
  subjectActive,
  professionalName,
  toggleRestDay,
  computedStudyDays,
  isStudyDaysValid,
  saving,
  savedFlash,
  showUnsavedModal,
  hasUnsavedChanges,
  discardAndLeave,
  cancelLeave,
  onUnsavedModalClose,
  textbookSaving,
  textbookSavedFlash,
  saveTextbook,
  syncFormFromStore,
  loadStudyState,
  handleSave,
  syncSavedSnapshot,
} = useSettingsForm();

// ── 左侧导航与区块滚动 ──
const { navSections, activeSection, scrollToSection, initSectionObserver } = useSectionNavigation();

onMounted(async () => {
  await settingsStore.load();
  syncFormFromStore();
  await loadStudyState();
  syncSavedSnapshot();
  initSectionObserver();

  // 如果 URL hash 指向 update 区块，自动滚动到"检查更新"区域并触发检查
  if (window.location.hash === "#settings-update") {
    setTimeout(() => {
      scrollToSection("update");
      void updateStore.checkUpdate();
    }, 200);
  }
});

// 监听 hash 变化（已在设置页内时再次点击侧边栏版本号）
watch(
  () => route.hash,
  (newHash) => {
    if (newHash === "#settings-update") {
      scrollToSection("update");
      void updateStore.checkUpdate();
    }
  }
);
</script>

<template>
  <div class="settings-view">
    <LoadingSpinner
      v-if="settingsStore.loading && !settingsStore.settings"
      :size="28"
      label="加载设置..."
    />

    <div v-else-if="settingsStore.settings && form" class="settings-container">
      <!-- 左侧快速导航栏 -->
      <nav class="settings-nav">
        <button
          v-for="s in navSections"
          :key="s.id"
          class="nav-item"
          :class="{ active: activeSection === s.id }"
          @click="scrollToSection(s.id)"
        >
          <component :is="s.icon" :size="16" />
          <span>{{ s.label }}</span>
        </button>
      </nav>

      <div class="settings-content">
        <PersonalInfoSection :form="form" />
        <GeneralSection />
        <AppearanceSection />
        <GoalsSection :form="form" />
        <ScheduleSection
          :form="form"
          :weekday-options="weekdayOptions"
          :toggle-rest-day="toggleRestDay"
          :computed-study-days="computedStudyDays"
          :is-study-days-valid="isStudyDaysValid"
        />
        <RhythmSection
          :form="form"
          :subject-active="subjectActive"
          :professional-name="professionalName"
        />
        <TimeAllocationSection
          :form="form"
          :study-state="studyState"
          :subject-active="subjectActive"
          :professional-name="professionalName"
        />
        <TextbooksSection
          :state-loading="stateLoading"
          :study-state="studyState"
          :subject-active="subjectActive"
          :professional-name="professionalName"
          :textbook-form="textbookForm"
          :textbook-saving="textbookSaving"
          :textbook-saved-flash="textbookSavedFlash"
          :state-error="stateError"
          :save-textbook="saveTextbook"
        />
        <AIProviderSection />
        <DidaSyncSection />

        <!-- MCP 配置区（暂时下线：MCP 适配暂不稳定，后续版本恢复） -->
        <McpSection v-if="false" :scroll-to-section="scrollToSection" />

        <StorageSection
          :sync-form-from-store="syncFormFromStore"
          :sync-saved-snapshot="syncSavedSnapshot"
        />
        <UpdateSection />

        <!-- 悬浮保存按钮 -->
        <div class="save-fab">
          <span v-if="hasUnsavedChanges" class="unsaved-hint">
            <AlertCircle :size="13" />
            有未保存的修改
          </span>
          <Button
            variant="primary"
            size="lg"
            :loading="saving"
            :disabled="!isStudyDaysValid || saving || savedFlash"
            class="save-btn"
            :class="{ 'saved': savedFlash }"
            @click="handleSave"
          >
            <span class="save-btn-content">
              <Check v-if="savedFlash" :size="16" class="saved-icon" />
              <Save v-else-if="!saving" :size="16" />
              <span v-if="savedFlash">已保存</span>
              <span v-else-if="!saving">保存设置</span>
              <span v-else>保存中...</span>
            </span>
          </Button>
        </div>
      </div>
    </div>

    <!-- 未保存修改确认弹窗 -->
    <Modal
      :open="showUnsavedModal"
      title="有未保存的修改"
      :close-on-overlay="false"
      :close-on-esc="false"
      @close="onUnsavedModalClose"
    >
      <p class="unsaved-modal-text">
        当前页面还有未保存的设置修改，离开后将丢失这些改动。确定要离开吗？
      </p>
      <template #footer>
        <Button variant="ghost" size="md" @click="cancelLeave">
          留在本页
        </Button>
        <Button variant="primary" size="md" @click="discardAndLeave">
          放弃修改并离开
        </Button>
      </template>
    </Modal>
  </div>
</template>
