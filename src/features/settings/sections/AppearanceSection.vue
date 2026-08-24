<script setup lang="ts">
import Card from "@/components/ui/Card.vue";
import { useSettingsStore } from "@/stores/settings";
import { Palette, Check, Droplet, RotateCcw, ImagePlus, Trash2 } from "lucide-vue-next";
import { useAppearanceSettings } from "../composables/useAppearanceSettings";

const settingsStore = useSettingsStore();

const {
  themeOptions,
  handleSetTheme,
  visualModeOptions,
  handleSetVisualMode,
  accentPresets,
  handleSetAccentColor,
  handleSetShowLogo,
  bgUploading,
  bgUploadError,
  handleUploadBackground,
  handleClearBackground,
  handleSetBackgroundBlur,
  handleSetBackgroundOpacity,
} = useAppearanceSettings();
</script>

<template>
  <!-- 外观配置区（紧随个人信息） -->
  <Card id="settings-appearance" padding="lg" class="settings-section">
    <div class="section-head">
      <div class="section-title">
        <Palette :size="18" />
        <span>外观</span>
      </div>
    </div>

    <div class="form-field">
      <label class="form-label">主题</label>
      <div class="theme-options">
        <button
          v-for="opt in themeOptions"
          :key="opt.mode"
          class="theme-card"
          :class="{ active: settingsStore.theme === opt.mode }"
          @click="handleSetTheme(opt.mode)"
        >
          <component :is="opt.icon" :size="20" class="theme-icon" />
          <span class="theme-label">{{ opt.label }}</span>
          <Check v-if="settingsStore.theme === opt.mode" :size="14" class="theme-check" />
        </button>
      </div>
    </div>

    <div class="form-field">
      <label class="form-label">视觉模式</label>
      <div class="visual-mode-options">
        <button
          v-for="opt in visualModeOptions"
          :key="opt.mode"
          class="visual-mode-card"
          :class="{ active: settingsStore.visualMode === opt.mode }"
          @click="handleSetVisualMode(opt.mode)"
        >
          <div class="visual-mode-header">
            <component :is="opt.icon" :size="20" class="visual-mode-icon" />
            <span class="visual-mode-label">{{ opt.label }}</span>
            <span v-if="opt.experimental" class="experimental-badge">实验性</span>
            <Check v-if="settingsStore.visualMode === opt.mode" :size="14" class="visual-mode-check" />
          </div>
          <span class="visual-mode-desc">{{ opt.desc }}</span>
        </button>
      </div>
    </div>

    <!-- 主色调色盘 -->
    <div class="form-field">
      <label class="form-label">主色调</label>
      <div class="accent-picker">
        <button
          v-for="preset in accentPresets"
          :key="preset.value"
          class="accent-swatch"
          :class="{ active: settingsStore.accentColor === preset.value }"
          :style="{ background: preset.value }"
          :title="preset.label"
          @click="handleSetAccentColor(preset.value)"
        >
          <Check v-if="settingsStore.accentColor === preset.value" :size="14" class="accent-check" />
        </button>
        <!-- 自定义颜色选择器 -->
        <label class="accent-custom" title="自定义颜色">
          <input
            type="color"
            :value="settingsStore.accentColor || '#5b8def'"
            class="accent-color-input"
            @input="handleSetAccentColor(($event.target as HTMLInputElement).value)"
          />
          <Droplet :size="14" />
        </label>
        <!-- 重置为默认 -->
        <button
          v-if="settingsStore.accentColor"
          class="accent-reset"
          title="恢复默认"
          @click="handleSetAccentColor('')"
        >
          <RotateCcw :size="13" />
          默认
        </button>
      </div>
    </div>

    <!-- Logo 显示开关 -->
    <div class="form-field">
      <div class="toggle-row">
        <div class="toggle-info">
          <label class="form-label">显示左上角 Logo</label>
          <span class="toggle-desc">关闭后侧边栏左上角只显示文字</span>
        </div>
        <button
          class="toggle-switch"
          :class="{ on: settingsStore.showLogo }"
          role="switch"
          :aria-checked="settingsStore.showLogo"
          @click="handleSetShowLogo(!settingsStore.showLogo)"
        >
          <span class="toggle-thumb" />
        </button>
      </div>
    </div>

    <!-- 自定义背景图 -->
    <div class="form-field">
      <label class="form-label">背景图</label>
      <span class="toggle-desc" style="margin-bottom: 8px; display: block;">
        上传图片作为应用背景，可调整模糊度与不透明度
      </span>
      <div class="background-picker">
        <button
          class="bg-upload-btn"
          :disabled="bgUploading"
          @click="handleUploadBackground"
        >
          <ImagePlus :size="16" />
          <span>{{ bgUploading ? '上传中…' : '选择图片' }}</span>
        </button>
        <button
          v-if="settingsStore.backgroundImage"
          class="bg-clear-btn"
          @click="handleClearBackground"
        >
          <Trash2 :size="14" />
          <span>移除背景</span>
        </button>
      </div>
      <div v-if="bgUploadError" class="bg-error">{{ bgUploadError }}</div>
      <div v-if="settingsStore.backgroundImage" class="bg-preview-row">
        <span class="bg-preview-label">当前背景：</span>
        <span class="bg-preview-path">{{ settingsStore.backgroundImage }}</span>
      </div>
    </div>

    <!-- 背景模糊度滑块（仅有背景图时显示） -->
    <div v-if="settingsStore.backgroundImage" class="form-field">
      <label class="form-label">
        模糊度
        <span class="slider-value">{{ settingsStore.backgroundBlur.toFixed(1) }}px</span>
      </label>
      <input
        type="range"
        min="0"
        max="20"
        step="0.5"
        :value="settingsStore.backgroundBlur"
        class="bg-slider"
        @input="handleSetBackgroundBlur(parseFloat(($event.target as HTMLInputElement).value))"
      />
    </div>

    <!-- 背景不透明度滑块（仅有背景图时显示） -->
    <div v-if="settingsStore.backgroundImage" class="form-field">
      <label class="form-label">
        不透明度
        <span class="slider-value">{{ Math.round(settingsStore.backgroundOpacity * 100) }}%</span>
      </label>
      <input
        type="range"
        min="0.1"
        max="1"
        step="0.05"
        :value="settingsStore.backgroundOpacity"
        class="bg-slider"
        @input="handleSetBackgroundOpacity(parseFloat(($event.target as HTMLInputElement).value))"
      />
    </div>
  </Card>
</template>
