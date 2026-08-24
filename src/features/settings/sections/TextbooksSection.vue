<script setup lang="ts">
import Card from "@/components/ui/Card.vue";
import Button from "@/components/ui/Button.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import { BookOpen, AlertCircle, Check, Save } from "lucide-vue-next";
import type { StudyState, SubjectKey } from "@/types/state";
import type { TextbookForm } from "../composables/useSettingsForm";
import type { SubjectActive } from "../types";

defineProps<{
  stateLoading: boolean;
  studyState: StudyState | null;
  subjectActive: SubjectActive;
  professionalName: string;
  textbookForm: TextbookForm;
  textbookSaving: Record<SubjectKey, boolean>;
  textbookSavedFlash: Record<SubjectKey, boolean>;
  stateError: string | null;
  saveTextbook: (subject: SubjectKey) => void;
}>();
</script>

<template>
  <!-- 教材配置区（独立保存） -->
  <Card id="settings-textbooks" padding="lg" class="settings-section">
    <div class="section-head">
      <div class="section-title">
        <BookOpen :size="18" />
        <span>教材</span>
      </div>
    </div>
    <p class="section-desc">
      每科教材独立保存（点击对应"保存"立即生效）。生成周计划时，AI 会联网检索教材目录，确保任务引用的章节名与小节编号与教材实际目录一致。
    </p>
    <LoadingSpinner v-if="stateLoading" :size="20" label="加载学习状态..." />
    <div v-else-if="studyState" class="textbook-list">
      <div v-if="subjectActive.math" class="textbook-row">
        <div class="textbook-info">
          <span class="textbook-label">数学</span>
          <span class="textbook-phase">{{ studyState.subjects.math.phase }}</span>
        </div>
        <div class="textbook-input-row">
          <input
            v-model="textbookForm.math"
            type="text"
            class="form-input"
            placeholder="如：张宇高数18讲（2026版）"
          />
          <Button
            variant="primary"
            size="sm"
            :loading="textbookSaving.math"
            :disabled="textbookSaving.math || textbookSavedFlash.math"
            @click="saveTextbook('math')"
          >
            <Check v-if="textbookSavedFlash.math" :size="14" />
            <Save v-else :size="14" />
            <span>{{ textbookSavedFlash.math ? '已保存' : '保存' }}</span>
          </Button>
        </div>
      </div>
      <div v-if="subjectActive.english" class="textbook-row">
        <div class="textbook-info">
          <span class="textbook-label">英语</span>
          <span class="textbook-phase">{{ studyState.subjects.english.phase }}</span>
        </div>
        <div class="textbook-input-row">
          <input
            v-model="textbookForm.english"
            type="text"
            class="form-input"
            placeholder="如：考研英语真题黄皮书"
          />
          <Button
            variant="primary"
            size="sm"
            :loading="textbookSaving.english"
            :disabled="textbookSaving.english || textbookSavedFlash.english"
            @click="saveTextbook('english')"
          >
            <Check v-if="textbookSavedFlash.english" :size="14" />
            <Save v-else :size="14" />
            <span>{{ textbookSavedFlash.english ? '已保存' : '保存' }}</span>
          </Button>
        </div>
      </div>
      <div v-if="subjectActive.politics" class="textbook-row">
        <div class="textbook-info">
          <span class="textbook-label">政治</span>
          <span class="textbook-phase">{{ studyState.subjects.politics.phase }}</span>
        </div>
        <div class="textbook-input-row">
          <input
            v-model="textbookForm.politics"
            type="text"
            class="form-input"
            placeholder="如：肖秀荣精讲精练"
          />
          <Button
            variant="primary"
            size="sm"
            :loading="textbookSaving.politics"
            :disabled="textbookSaving.politics || textbookSavedFlash.politics"
            @click="saveTextbook('politics')"
          >
            <Check v-if="textbookSavedFlash.politics" :size="14" />
            <Save v-else :size="14" />
            <span>{{ textbookSavedFlash.politics ? '已保存' : '保存' }}</span>
          </Button>
        </div>
      </div>
      <div v-if="subjectActive.professional" class="textbook-row">
        <div class="textbook-info">
          <span class="textbook-label">{{ professionalName }}</span>
          <span class="textbook-phase">{{ studyState.subjects.professional.phase }}</span>
        </div>
        <div class="textbook-input-row">
          <input
            v-model="textbookForm.professional"
            type="text"
            class="form-input"
            placeholder="如：王道408 2026版"
          />
          <Button
            variant="primary"
            size="sm"
            :loading="textbookSaving.professional"
            :disabled="textbookSaving.professional || textbookSavedFlash.professional"
            @click="saveTextbook('professional')"
          >
            <Check v-if="textbookSavedFlash.professional" :size="14" />
            <Save v-else :size="14" />
            <span>{{ textbookSavedFlash.professional ? '已保存' : '保存' }}</span>
          </Button>
        </div>
      </div>
      <div
        v-if="!subjectActive.math && !subjectActive.english && !subjectActive.politics && !subjectActive.professional"
        class="empty-inline"
      >
        尚未启用任何科目。完成首次配置后会自动激活各科。
      </div>
    </div>
    <div v-if="stateError" class="dir-change-msg error">
      <AlertCircle :size="14" />
      <span>{{ stateError }}</span>
    </div>
  </Card>
</template>
