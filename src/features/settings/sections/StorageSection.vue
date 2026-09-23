<script setup lang="ts">
import Card from "@/components/ui/Card.vue";
import Button from "@/components/ui/Button.vue";
import { useSettingsStore } from "@/stores/settings";
import { useBackupActions } from "../composables/useBackupActions";
import { FolderOpen, Download, Package, AlertCircle, CheckCircle } from "lucide-vue-next";

const props = defineProps<{
  /** 数据目录切换后刷新表单缓冲 */
  syncFormFromStore: () => void;
  /** 数据目录切换后重置「未保存」快照基准 */
  syncSavedSnapshot: () => void;
}>();

const settingsStore = useSettingsStore();

const {
  changingDir,
  dirChangeMsg,
  dirChangeError,
  handleChangeDataDir,
  exporting,
  importing,
  backupMsg,
  backupError,
  handleExportBackup,
  handleImportBackup,
} = useBackupActions({
  syncFormFromStore: () => props.syncFormFromStore(),
  syncSavedSnapshot: () => props.syncSavedSnapshot(),
});
</script>

<template>
  <!-- 存储配置区 -->
  <Card id="settings-storage" padding="lg" class="settings-section">
    <div class="section-head">
      <div class="section-title">
        <FolderOpen :size="18" />
        <span>存储</span>
      </div>
    </div>
    <div class="form-field form-field-full">
      <label class="form-label">数据目录</label>
      <div class="data-dir">
        <FolderOpen :size="15" class="dir-icon" />
        <span class="dir-path">{{ settingsStore.dataDirectory || "未设置" }}</span>
        <Button
          variant="secondary"
          size="sm"
          :loading="changingDir"
          class="dir-change-btn"
          @click="handleChangeDataDir"
        >
          <FolderOpen :size="14" />
          <span>更改目录</span>
        </Button>
      </div>
      <p class="field-hint">
        数据目录存储学习计划、复盘、进度表与自适应状态。更改目录前请先导出备份，切换后再导入；
        这样可以完整迁移所有已注册的数据目录。更改后立即生效，重启后保留。
      </p>
      <div v-if="dirChangeMsg" class="dir-change-msg" :class="{ error: dirChangeError }">
        <component :is="dirChangeError ? AlertCircle : CheckCircle" :size="14" />
        <span>{{ dirChangeMsg }}</span>
      </div>
    </div>

    <!-- 数据备份 / 导出 / 导入 -->
    <div class="form-field form-field-full backup-field">
      <label class="form-label">数据备份</label>
      <div class="backup-actions">
        <Button
          variant="secondary"
          size="md"
          :loading="exporting"
          :disabled="importing"
          @click="handleExportBackup"
        >
          <Download :size="15" />
          <span>导出备份（zip）</span>
        </Button>
        <Button
          variant="secondary"
          size="md"
          :loading="importing"
          :disabled="exporting"
          @click="handleImportBackup"
        >
          <Package :size="15" />
          <span>导入恢复</span>
        </Button>
      </div>
      <p class="field-hint">
        导出会打包学习计划、复盘、状态、进度表、自适应规划、设置与教材数据，并写入逐文件完整性校验；
        导入会先验证备份，再自动保留当前数据目录，完成后需重启应用生效。
      </p>
      <div v-if="backupMsg" class="backup-msg" :class="{ error: backupError }">
        <component :is="backupError ? AlertCircle : CheckCircle" :size="14" />
        <span>{{ backupMsg }}</span>
      </div>
    </div>
  </Card>
</template>
