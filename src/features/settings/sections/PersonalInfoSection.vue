<script setup lang="ts">
import Card from "@/components/ui/Card.vue";
import { User } from "lucide-vue-next";
import type { SettingsForm } from "../composables/useSettingsForm";

defineProps<{
  form: SettingsForm;
}>();
</script>

<template>
  <!-- 个人信息配置区 -->
  <Card id="settings-personal" padding="lg" class="settings-section">
    <div class="section-head">
      <div class="section-title">
        <User :size="18" />
        <span>个人信息</span>
      </div>
    </div>
    <div class="form-grid">
      <div class="form-field form-field-full">
        <label class="form-label">用户称呼</label>
        <input
          v-model="form.user_name"
          type="text"
          class="form-input"
          placeholder="如：小明（用于首页问候）"
        />
      </div>
      <div class="form-field form-field-full">
        <div class="toggle-row">
          <div class="toggle-text">
            <span class="toggle-title">首页问候显示</span>
            <span class="toggle-desc">在工作台顶部显示时间问候与称呼</span>
          </div>
          <button
            class="toggle-switch"
            :class="{ on: form.show_greeting }"
            role="switch"
            :aria-checked="form.show_greeting"
            @click="form.show_greeting = !form.show_greeting"
          >
            <span class="toggle-thumb" />
          </button>
        </div>
      </div>
    </div>
  </Card>
</template>
