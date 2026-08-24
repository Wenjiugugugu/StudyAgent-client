<script setup lang="ts">
import Card from "@/components/ui/Card.vue";
import Button from "@/components/ui/Button.vue";
import Badge from "@/components/ui/Badge.vue";
import Select from "@/components/ui/Select.vue";
import { useSettingsStore } from "@/stores/settings";
import { useMcpServerEditor } from "../composables/useMcpServerEditor";
import { Server, Plus, Trash2, Check, Pencil, HelpCircle } from "lucide-vue-next";

const props = defineProps<{
  /** 用于「使用示例」时跳转到 MCP Server 区块 */
  scrollToSection: (id: string) => void;
}>();

const settingsStore = useSettingsStore();

const {
  showServerForm,
  editingServerId,
  serverArgsText,
  serverForm,
  mcpTypeOptions,
  transportOptions,
  startAddServer,
  editServer,
  cancelServerForm,
  saveServer,
  removeServer,
  toggleServerEnabled,
  showMcpTips,
  MCP_TIPS,
  applyMcpTip,
} = useMcpServerEditor({
  scrollToSection: (id) => props.scrollToSection(id),
});
</script>

<template>
  <!-- MCP 配置区（暂时下线：MCP 适配暂不稳定，后续版本恢复） -->
  <Card id="settings-mcp-server" padding="lg" class="settings-section">
    <div class="section-head">
      <div class="section-title">
        <Server :size="18" />
        <span>MCP Server</span>
      </div>
      <Button variant="secondary" size="sm" @click="startAddServer">
        <Plus :size="14" />
        <span>添加</span>
      </Button>
    </div>

    <div class="item-list">
      <div
        v-for="server in settingsStore.mcpServers"
        :key="server.id"
        class="item-row"
      >
        <div class="item-info">
          <div class="item-name-row">
            <span class="item-name">{{ server.name }}</span>
            <Badge :variant="server.enabled ? 'success' : 'default'">
              {{ server.enabled ? "已启用" : "已禁用" }}
            </Badge>
          </div>
          <div class="item-sub">
            <span>{{ server.type }}</span>
            <span>· {{ server.transport }}</span>
            <span v-if="server.command">· {{ server.command }}</span>
          </div>
        </div>
        <div class="item-actions">
          <Button variant="ghost" size="sm" @click="toggleServerEnabled(server)">
            {{ server.enabled ? "禁用" : "启用" }}
          </Button>
          <Button variant="ghost" size="sm" icon @click="editServer(server)">
            <Pencil :size="14" />
          </Button>
          <Button variant="ghost" size="sm" icon @click="removeServer(server.id)">
            <Trash2 :size="14" />
          </Button>
        </div>
      </div>

      <div v-if="settingsStore.mcpServers.length === 0" class="empty-inline">
        尚未配置 MCP Server。
      </div>
    </div>

    <div v-if="showServerForm" class="edit-form">
      <div class="form-title">
        {{ editingServerId ? "编辑 Server" : "新增 Server" }}
      </div>
      <div class="form-grid">
        <div class="form-field">
          <label class="form-label">名称</label>
          <input v-model="serverForm.name" type="text" class="form-input" placeholder="文件系统" />
        </div>
        <div class="form-field">
          <label class="form-label">类型</label>
          <Select v-model="serverForm.type">
            <option v-for="opt in mcpTypeOptions" :key="opt.value" :value="opt.value">
              {{ opt.label }}
            </option>
          </Select>
        </div>
        <div class="form-field">
          <label class="form-label">传输方式</label>
          <Select v-model="serverForm.transport">
            <option v-for="opt in transportOptions" :key="opt.value" :value="opt.value">
              {{ opt.label }}
            </option>
          </Select>
        </div>
        <div class="form-field">
          <label class="form-label">命令 (stdio)</label>
          <input v-model="serverForm.command" type="text" class="form-input" placeholder="npx" />
        </div>
        <div class="form-field form-field-full">
          <label class="form-label">参数（逗号分隔）</label>
          <input v-model="serverArgsText" type="text" class="form-input" placeholder="-y, @modelcontextprotocol/server-filesystem, ." />
        </div>
        <div class="form-field form-field-full">
          <label class="form-label">URL (SSE / WebSocket)</label>
          <input v-model="serverForm.url" type="text" class="form-input" placeholder="http://localhost:3000/sse" />
        </div>
      </div>

      <div class="form-actions">
        <Button variant="ghost" size="sm" @click="cancelServerForm">取消</Button>
        <Button variant="primary" size="sm" @click="saveServer">
          <Check :size="14" />
          <span>保存</span>
        </Button>
      </div>
    </div>

    <!-- MCP 配置小贴士 -->
    <div class="mcp-tips-block">
      <button
        type="button"
        class="mcp-tips-toggle"
        :class="{ expanded: showMcpTips }"
        @click="showMcpTips = !showMcpTips"
      >
        <HelpCircle :size="14" />
        <span>不知道填什么？查看常用 MCP Server 配置示例</span>
      </button>
      <transition name="mcp-tips-fade">
        <div v-if="showMcpTips" class="mcp-tips-list">
          <div
            v-for="(tip, idx) in MCP_TIPS"
            :key="idx"
            class="mcp-tip-card"
          >
            <div class="mcp-tip-info">
              <div class="mcp-tip-name">{{ tip.name }}</div>
              <div class="mcp-tip-desc">{{ tip.desc }}</div>
              <div class="mcp-tip-cmd">
                <code>{{ tip.command }} {{ tip.args }}</code>
              </div>
            </div>
            <Button variant="secondary" size="sm" @click="applyMcpTip(tip)">
              <Plus :size="12" />
              <span>使用</span>
            </Button>
          </div>
          <p class="mcp-tips-note">
            以上配置需先安装 Node.js（npx）或 Python + uvx。命令执行需要联网下载对应 MCP Server。
          </p>
        </div>
      </transition>
    </div>
  </Card>
</template>
