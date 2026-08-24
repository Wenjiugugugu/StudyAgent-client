/**
 * 设置页 — MCP Server 编辑逻辑（当前版本该区块暂未渲染，逻辑完整保留）
 *
 * 原 SettingsView 中 showServerForm / serverForm / startAddServer / saveServer 等，
 * 以及 MCP 配置小贴士（showMcpTips / MCP_TIPS / applyMcpTip）。
 * applyMcpTip 需要跳转到 mcp-server 区块，因此依赖 useSectionNavigation 的 scrollToSection，
 * 以参数形式注入。
 */
import { ref } from "vue";
import { useSettingsStore } from "@/stores/settings";
import type { MCPServerConfig, MCPServerType } from "@/types";

export interface McpServerEditorDeps {
  scrollToSection: (id: string) => void;
}

interface McpTip {
  name: string;
  desc: string;
  type: MCPServerType;
  transport: "stdio" | "sse" | "websocket";
  command: string;
  args: string;
  url: string;
}

export function useMcpServerEditor(deps: McpServerEditorDeps) {
  const settingsStore = useSettingsStore();

  // ── MCP Server 表单状态 ──
  const showServerForm = ref(false);
  const editingServerId = ref<string | null>(null);
  const serverArgsText = ref("");

  function emptyServer(): MCPServerConfig {
    return {
      id: "",
      name: "",
      type: "filesystem",
      enabled: true,
      transport: "stdio",
      command: "",
      args: [],
      url: "",
    };
  }

  const serverForm = ref<MCPServerConfig>(emptyServer());

  const mcpTypeOptions: { value: MCPServerType; label: string }[] = [
    { value: "filesystem", label: "文件系统" },
    { value: "browser", label: "浏览器" },
    { value: "obsidian", label: "Obsidian" },
    { value: "custom", label: "自定义" },
  ];

  const transportOptions: { value: "stdio" | "sse" | "websocket"; label: string }[] = [
    { value: "stdio", label: "STDIO" },
    { value: "sse", label: "SSE" },
    { value: "websocket", label: "WebSocket" },
  ];

  function startAddServer() {
    editingServerId.value = null;
    serverForm.value = emptyServer();
    serverArgsText.value = "";
    showServerForm.value = true;
  }

  function editServer(s: MCPServerConfig) {
    editingServerId.value = s.id;
    serverForm.value = { ...s, args: [...(s.args ?? [])] };
    serverArgsText.value = (s.args ?? []).join(", ");
    showServerForm.value = true;
  }

  function cancelServerForm() {
    showServerForm.value = false;
    editingServerId.value = null;
  }

  function saveServer() {
    if (!serverForm.value.name.trim()) return;
    serverForm.value.args = serverArgsText.value
      .split(",")
      .map((a) => a.trim())
      .filter(Boolean);
    if (editingServerId.value) {
      settingsStore.updateMCPServer(editingServerId.value, { ...serverForm.value });
    } else {
      settingsStore.addMCPServer({
        ...serverForm.value,
        id: `mcp-${Date.now()}`,
      });
    }
    showServerForm.value = false;
    editingServerId.value = null;
  }

  function removeServer(id: string) {
    settingsStore.removeMCPServer(id);
  }

  function toggleServerEnabled(s: MCPServerConfig) {
    settingsStore.updateMCPServer(s.id, { enabled: !s.enabled });
  }

  // ── MCP 配置示例（小贴士） ──
  const showMcpTips = ref(false);

  const MCP_TIPS: McpTip[] = [
    {
      name: "文件系统",
      desc: "让 AI 读写本地目录文件（官方推荐起步）",
      type: "filesystem",
      transport: "stdio",
      command: "npx",
      args: "-y, @modelcontextprotocol/server-filesystem, .",
      url: "",
    },
    {
      name: "GitHub",
      desc: "查询仓库、Issue、PR 等",
      type: "custom",
      transport: "stdio",
      command: "npx",
      args: "-y, @modelcontextprotocol/server-github",
      url: "",
    },
    {
      name: "Fetch 网页抓取",
      desc: "抓取网页内容供 AI 阅读",
      type: "custom",
      transport: "stdio",
      command: "uvx",
      args: "mcp-server-fetch",
      url: "",
    },
  ];

  function applyMcpTip(tip: McpTip) {
    // 跳转到 MCP Server 区块并预填表单
    deps.scrollToSection("mcp-server");
    startAddServer();
    serverForm.value.name = tip.name;
    serverForm.value.type = tip.type;
    serverForm.value.transport = tip.transport;
    serverForm.value.command = tip.command;
    serverArgsText.value = tip.args;
    serverForm.value.url = tip.url;
  }

  return {
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
  };
}
