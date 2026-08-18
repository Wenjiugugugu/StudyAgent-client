<script setup lang="ts">
/**
 * 轻量级 Markdown 渲染器
 *
 * 支持 GitHub Flavored Markdown 常用子集：
 * - 标题（# ~ ######）
 * - 段落
 * - 粗体 **text** / 斜体 *text*
 * - 行内代码 `code`
 * - 代码块 ```lang ... ```
 * - 无序列表 (- / *) 与有序列表 (1.)
 * - 链接 [text](url)
 * - 水平分割线 ---
 * - 引用 > text
 *
 * 不引入第三方依赖。所有文本节点先 HTML 转义，再做 Markdown 替换，避免 XSS。
 */
import { computed } from "vue";

const props = defineProps<{
  content: string;
}>();

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/** 行内代码占位符：使用 \u0001 控制字符 + 序号，避免被字符串处理过程截断（不依赖 NUL） */
const CODE_PLACEHOLDER = (idx: number): string => `\u0001CODE_${idx}\u0001`;

/**
 * 渲染行内 Markdown：粗体、斜体、行内代码、链接。
 * 输入应为已转义的 HTML 文本（& < > 已变为实体）。
 */
function renderInline(s: string): string {
  // 行内代码：用占位符保护，避免后续替换破坏内部内容
  const codePlaceholders: string[] = [];
  let text = s.replace(/`([^`]+)`/g, (_m, code: string) => {
    const idx = codePlaceholders.length;
    // code 已被外部 escapeHtml 转义，直接包标签
    codePlaceholders.push(`<code class="md-inline-code">${code}</code>`);
    return CODE_PLACEHOLDER(idx);
  });

  // 链接 [text](url) — label 已转义；url 提取后再转义
  text = text.replace(
    /\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g,
    (_m, label: string, url: string) =>
      `<a href="${escapeHtml(url)}" target="_blank" rel="noopener noreferrer">${label}</a>`,
  );

  // 粗体 **text**
  text = text.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  // 斜体 *text*（避免与粗体冲突）
  text = text.replace(/(^|[^*])\*([^*]+)\*(?!\*)/g, "$1<em>$2</em>");

  // 还原行内代码占位符
  text = text.replace(/\u0001CODE_(\d+)\u0001/g, (_m, idx: string) =>
    codePlaceholders[parseInt(idx, 10)] ?? "",
  );

  return text;
}

const html = computed(() => {
  const src = props.content ?? "";
  if (!src.trim()) return "";

  const lines = src.replace(/\r\n/g, "\n").split("\n");
  const out: string[] = [];
  let i = 0;

  let inUl = false;
  let inOl = false;
  let inQuote = false;
  let inCode = false;
  let codeLang = "";
  const codeBuf: string[] = [];

  function closeLists() {
    if (inUl) { out.push("</ul>"); inUl = false; }
    if (inOl) { out.push("</ol>"); inOl = false; }
  }
  function closeQuote() {
    if (inQuote) { out.push("</blockquote>"); inQuote = false; }
  }

  while (i < lines.length) {
    const line = lines[i];

    // 代码块围栏
    const fence = /^```(.*)$/.exec(line);
    if (fence) {
      if (!inCode) {
        closeLists();
        closeQuote();
        inCode = true;
        codeLang = fence[1]?.trim() ?? "";
        codeBuf.length = 0;
      } else {
        const code = escapeHtml(codeBuf.join("\n"));
        out.push(
          `<pre class="md-code-block"><code${
            codeLang ? ` class="language-${escapeHtml(codeLang)}"` : ""
          }>${code}</code></pre>`,
        );
        inCode = false;
        codeLang = "";
      }
      i++;
      continue;
    }
    if (inCode) {
      codeBuf.push(line);
      i++;
      continue;
    }

    // 空行
    if (line.trim() === "") {
      closeLists();
      closeQuote();
      i++;
      continue;
    }

    // 水平分割线
    if (/^\s*([-*_])\1{2,}\s*$/.test(line)) {
      closeLists();
      closeQuote();
      out.push("<hr />");
      i++;
      continue;
    }

    // 标题
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      const level = heading[1].length;
      closeLists();
      closeQuote();
      out.push(
        `<h${level} class="md-h md-h${level}">${renderInline(escapeHtml(heading[2]))}</h${level}>`,
      );
      i++;
      continue;
    }

    // 引用
    const quote = /^>\s?(.*)$/.exec(line);
    if (quote) {
      closeLists();
      if (!inQuote) {
        out.push('<blockquote class="md-quote">');
        inQuote = true;
      }
      out.push(`<p>${renderInline(escapeHtml(quote[1]))}</p>`);
      i++;
      continue;
    } else {
      closeQuote();
    }

    // 无序列表
    const ul = /^[\s]*[-*+]\s+(.*)$/.exec(line);
    if (ul) {
      if (!inUl) { closeLists(); out.push('<ul class="md-ul">'); inUl = true; }
      out.push(`<li>${renderInline(escapeHtml(ul[1]))}</li>`);
      i++;
      continue;
    }

    // 有序列表
    const ol = /^[\s]*\d+\.\s+(.*)$/.exec(line);
    if (ol) {
      if (!inOl) { closeLists(); out.push('<ol class="md-ol">'); inOl = true; }
      out.push(`<li>${renderInline(escapeHtml(ol[1]))}</li>`);
      i++;
      continue;
    }

    // 普通段落
    closeLists();
    out.push(`<p class="md-p">${renderInline(escapeHtml(line))}</p>`);
    i++;
  }

  // 收尾
  if (inCode) {
    const code = escapeHtml(codeBuf.join("\n"));
    out.push(`<pre class="md-code-block"><code>${code}</code></pre>`);
  }
  closeLists();
  closeQuote();

  return out.join("\n");
});
</script>

<template>
  <div class="markdown-text" v-html="html"></div>
</template>

<style scoped>
.markdown-text {
  font-size: var(--text-sm);
  color: var(--text-secondary);
  line-height: var(--leading-relaxed);
  word-break: break-word;
}

.markdown-text :deep(.md-h) {
  color: var(--text-primary);
  font-weight: var(--font-semibold);
  margin: 0.8em 0 0.4em;
  letter-spacing: -0.01em;
}

.markdown-text :deep(.md-h1) { font-size: 1.15em; }
.markdown-text :deep(.md-h2) { font-size: 1.1em; }
.markdown-text :deep(.md-h3) { font-size: 1.05em; }
.markdown-text :deep(.md-h4),
.markdown-text :deep(.md-h5),
.markdown-text :deep(.md-h6) { font-size: 1em; }

.markdown-text :deep(.md-p) {
  margin: 0.3em 0;
}

.markdown-text :deep(.md-ul),
.markdown-text :deep(.md-ol) {
  margin: 0.4em 0;
  padding-left: 1.4em;
}

.markdown-text :deep(.md-ul) { list-style: disc; }
.markdown-text :deep(.md-ol) { list-style: decimal; }

.markdown-text :deep(.md-ul li),
.markdown-text :deep(.md-ol li) {
  margin: 0.2em 0;
}

.markdown-text :deep(.md-quote) {
  margin: 0.5em 0;
  padding: 0.3em 0.8em;
  border-left: 3px solid var(--border-color);
  color: var(--text-tertiary);
  background: var(--bg-tertiary);
  border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
}

.markdown-text :deep(.md-quote p) {
  margin: 0.2em 0;
}

.markdown-text :deep(.md-inline-code) {
  font-family: var(--font-mono);
  font-size: 0.9em;
  background: var(--bg-tertiary);
  padding: 1px 5px;
  border-radius: var(--radius-xs);
  border: 1px solid var(--border-color);
}

.markdown-text :deep(.md-code-block) {
  margin: 0.5em 0;
  padding: var(--space-3);
  background: var(--bg-tertiary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  overflow-x: auto;
  font-family: var(--font-mono);
  font-size: 0.85em;
  color: var(--text-primary);
}

.markdown-text :deep(.md-code-block code) {
  background: transparent;
  border: none;
  padding: 0;
}

.markdown-text :deep(a) {
  color: var(--accent);
  text-decoration: none;
}

.markdown-text :deep(a:hover) {
  text-decoration: underline;
}

.markdown-text :deep(strong) {
  color: var(--text-primary);
  font-weight: var(--font-semibold);
}

.markdown-text :deep(hr) {
  border: none;
  border-top: 1px solid var(--border-color);
  margin: 0.8em 0;
}
</style>
