<script setup lang="ts">
import { onMounted } from "vue";
import Card from "@/components/ui/Card.vue";
import { PowerOff, HelpCircle, Minimize2, Power, Check } from "lucide-vue-next";
import { useGeneralSettings } from "../composables/useGeneralSettings";

const {
  autostartEnabled,
  autostartLoading,
  closeAction,
  closeActionLoading,
  isTauriEnv,
  toggleAutostart,
  handleChangeCloseAction,
  loadGeneralSettings,
} = useGeneralSettings();

// 原 SettingsView onMounted 中调用；保持挂载即加载的行为
onMounted(() => {
  void loadGeneralSettings();
});
</script>

<template>
  <!-- 通用配置区：开机启动 / 关闭动作 -->
  <Card id="settings-general" padding="lg" class="settings-section">
    <div class="section-head">
      <div class="section-title">
        <PowerOff :size="18" />
        <span>通用</span>
      </div>
    </div>

    <!-- 开机启动 -->
    <div class="toggle-row">
      <div class="toggle-text">
        <span class="toggle-title">开机启动</span>
        <span class="toggle-desc">登录 Windows 后自动启动 StudyAgent</span>
      </div>
      <button
        class="toggle-switch"
        :class="{ on: autostartEnabled }"
        role="switch"
        :aria-checked="autostartEnabled"
        :disabled="autostartLoading || !isTauriEnv"
        @click="toggleAutostart(!autostartEnabled)"
      >
        <span class="toggle-thumb" />
      </button>
    </div>

    <!-- 关闭动作 -->
    <div class="form-field">
      <label class="form-label">关闭窗口时</label>
      <div class="close-action-options">
        <button
          type="button"
          class="close-action-option"
          :class="{ active: closeAction === 'ask' }"
          :disabled="closeActionLoading || !isTauriEnv"
          @click="handleChangeCloseAction('ask')"
        >
          <HelpCircle :size="18" class="close-action-icon" />
          <div class="close-action-text">
            <span class="close-action-label">每次询问</span>
            <span class="close-action-desc">关闭时弹窗选择</span>
          </div>
          <Check v-if="closeAction === 'ask'" :size="14" class="close-action-check" />
        </button>
        <button
          type="button"
          class="close-action-option"
          :class="{ active: closeAction === 'tray' }"
          :disabled="closeActionLoading || !isTauriEnv"
          @click="handleChangeCloseAction('tray')"
        >
          <Minimize2 :size="18" class="close-action-icon" />
          <div class="close-action-text">
            <span class="close-action-label">最小化到托盘</span>
            <span class="close-action-desc">保持后台运行</span>
          </div>
          <Check v-if="closeAction === 'tray'" :size="14" class="close-action-check" />
        </button>
        <button
          type="button"
          class="close-action-option"
          :class="{ active: closeAction === 'quit' }"
          :disabled="closeActionLoading || !isTauriEnv"
          @click="handleChangeCloseAction('quit')"
        >
          <Power :size="18" class="close-action-icon" />
          <div class="close-action-text">
            <span class="close-action-label">直接退出</span>
            <span class="close-action-desc">完全关闭应用</span>
          </div>
          <Check v-if="closeAction === 'quit'" :size="14" class="close-action-check" />
        </button>
      </div>
      <p v-if="!isTauriEnv" class="field-hint">
        当前环境不支持系统设置（仅在桌面应用中可用）
      </p>
    </div>
  </Card>
</template>
