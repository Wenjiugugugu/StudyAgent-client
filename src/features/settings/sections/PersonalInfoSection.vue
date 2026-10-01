<script setup lang="ts">
import Card from "@/components/ui/Card.vue";
import Switch from "@/components/ui/Switch.vue";
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
          <Switch v-model="form.show_greeting" label="首页问候显示" />
        </div>
      </div>
    </div>
  </Card>
</template>
