<script setup lang="ts">
/**
 * 调试页 — 数据文件检查面板
 *
 * 检查 state/plan/records/config 目录是否存在并列出文件，支持展开目录、
 * 预览文件内容。刷新依赖父页面提供的数据目录。
 */
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import CodeBlock from "@/components/CodeBlock.vue";
import {
  RefreshCw,
  FolderTree,
  FileText,
  ChevronRight,
  ChevronDown,
} from "lucide-vue-next";
import { useDataDirectoryCheck } from "../composables/useDataDirectoryCheck";

const props = defineProps<{ dataDir: string }>();

const { dataDirs, expandedDir, fileContent, loadingFile, checkDataDirs, toggleDir, viewFile } =
  useDataDirectoryCheck(() => props.dataDir);

defineExpose({ refresh: checkDataDirs });
</script>

<template>
  <Card id="debug-files" padding="lg" class="debug-section">
    <div class="section-head">
      <div class="section-title">
        <FolderTree :size="18" />
        <span>数据文件检查</span>
      </div>
      <Button variant="ghost" size="sm" @click="checkDataDirs">
        <RefreshCw :size="14" />
        <span>重新检查</span>
      </Button>
    </div>

    <div v-if="!dataDir" class="empty-inline">未设置数据目录，无法检查文件。</div>

    <div v-else class="dir-list">
      <div v-for="dir in dataDirs" :key="dir.name" class="dir-item">
        <button class="dir-header" @click="toggleDir(dir.name)">
          <component
            :is="expandedDir === dir.name ? ChevronDown : ChevronRight"
            :size="14"
            class="chevron"
          />
          <FileText :size="14" class="dir-icon" />
          <span class="dir-name">{{ dir.label }}</span>
          <Badge v-if="dir.loading" variant="info" size="sm">检查中</Badge>
          <Badge v-else-if="dir.exists === true" variant="success" size="sm">
            {{ dir.entries.length }} 个文件
          </Badge>
          <Badge v-else-if="dir.exists === false" variant="danger" size="sm">缺失</Badge>
        </button>

        <div v-if="expandedDir === dir.name" class="dir-content">
          <div v-if="dir.error" class="error-text">{{ dir.error }}</div>
          <div v-else-if="dir.entries.length === 0" class="empty-inline">目录为空</div>
          <ul v-else class="file-list">
            <li v-for="entry in dir.entries" :key="entry.name">
              <button
                class="file-item"
                :class="{ active: fileContent?.dir === dir.name && fileContent?.name === entry.name }"
                @click="viewFile(dir.name, entry)"
              >
                <FileText :size="13" class="file-icon" />
                <span class="file-name">{{ entry.name }}</span>
                <span v-if="entry.isDirectory" class="file-tag">目录</span>
              </button>
            </li>
          </ul>
        </div>
      </div>
    </div>

    <!-- 文件内容预览 -->
    <div v-if="fileContent" class="file-preview">
      <div class="preview-head">
        <span class="preview-title text-mono">{{ fileContent.dir }}/{{ fileContent.name }}</span>
        <Button variant="ghost" size="sm" icon @click="fileContent = null">
          ×
        </Button>
      </div>
      <LoadingSpinner v-if="loadingFile" :size="20" label="读取文件..." />
      <div v-else-if="fileContent.error" class="error-text">{{ fileContent.error }}</div>
      <CodeBlock v-else :code="fileContent.content" :label="`${fileContent.dir}/${fileContent.name}`" />
    </div>
  </Card>
</template>
