/** A deliberately small, HTML-safe Markdown subset used by local textbook content. */
export function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function safeLink(value: string): string | null {
  const trimmed = value.trim();
  if (/["'<>\u0000-\u001f]/.test(trimmed)) return null;
  if (!/^(https?:\/\/|mailto:)/i.test(trimmed)) return null;
  try {
    const url = new URL(trimmed);
    return ["http:", "https:", "mailto:"].includes(url.protocol) ? escapeHtml(url.href) : null;
  } catch {
    return null;
  }
}

function renderInline(raw: string, highlightTerms: string[] = []): string {
  const placeholders: string[] = [];
  const reserve = (html: string) => {
    const token = `\u0001INLINE_${placeholders.length}\u0001`;
    placeholders.push(html);
    return token;
  };

  let text = raw.replace(/`([^`]+)`/g, (_match, code: string) =>
    reserve(`<code class="md-code">${escapeHtml(code)}</code>`),
  );
  text = text.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_match, label: string, href: string) => {
    const safe = safeLink(href);
    return safe
      ? reserve(`<a href="${safe}" target="_blank" rel="noopener noreferrer" class="md-link">${escapeHtml(label)}</a>`)
      : label;
  });

  // M9：高亮在转义**前**对原文匹配——此时行内代码/链接已替换为占位符（不参与匹配），
  // 避免在转义后的文本上误命中 HTML 实体（如搜索 "amp"）或嵌套 <mark>
  for (const term of highlightTerms) {
    const trimmed = term.trim();
    if (!trimmed) continue;
    text = text.replace(new RegExp(`(${escapeRegExp(trimmed)})`, "gi"), (_match, m: string) =>
      reserve(`<mark class="md-hit">${escapeHtml(m)}</mark>`),
    );
  }

  text = escapeHtml(text);

  text = text.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  text = text.replace(/(^|[^*])\*([^*\n]+)\*(?!\*)/g, "$1<em>$2</em>");
  return text.replace(/\u0001INLINE_(\d+)\u0001/g, (_match, index: string) => placeholders[Number(index)] ?? "");
}

export interface MarkdownRenderOptions {
  lineNumbers?: boolean;
  headingPrefix?: string;
  activeLine?: number;
  highlightTerms?: string[];
}

export function slugifyHeading(text: string, prefix = "tb-"): string {
  return prefix + text.toLowerCase().replace(/[^\w\u4e00-\u9fa5]+/g, "-").replace(/^-+|-+$/g, "");
}

export function renderMarkdown(source: string, options: MarkdownRenderOptions = {}): string {
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  const output: string[] = [];
  let inCode = false;
  let codeLanguage = "";
  let codeStartLine = 1;
  let code: string[] = [];
  let list: "ul" | "ol" | null = null;
  const lineAttribute = (line: number) => options.lineNumbers ? ` data-line="${line}"` : "";
  const terms = (line: number) => options.activeLine === line ? options.highlightTerms ?? [] : [];
  const closeList = () => {
    if (list) output.push(`</${list}>`);
    list = null;
  };

  lines.forEach((line, index) => {
    const lineNumber = index + 1;
    const fence = line.match(/^```([\w-]*)\s*$/);
    if (fence) {
      if (!inCode) {
        closeList();
        inCode = true;
        codeLanguage = fence[1] || "";
        codeStartLine = lineNumber;
        code = [];
      } else {
        const languageClass = codeLanguage ? ` language-${escapeHtml(codeLanguage)}` : "";
        output.push(`<pre${lineAttribute(codeStartLine)} class="md-pre"><code class="md-block-code${languageClass}">${escapeHtml(code.join("\n"))}</code></pre>`);
        inCode = false;
      }
      return;
    }
    if (inCode) {
      code.push(line);
      return;
    }
    if (!line.trim()) {
      closeList();
      return;
    }

    const heading = line.match(/^(#{1,6})\s+(.+)$/);
    if (heading) {
      closeList();
      const level = heading[1].length;
      const text = heading[2].trim();
      const id = options.headingPrefix === undefined ? "" : ` id="${slugifyHeading(text, options.headingPrefix)}"`;
      output.push(`<h${level}${lineAttribute(lineNumber)}${id} class="md-h md-h${level}">${renderInline(text, terms(lineNumber))}</h${level}>`);
      return;
    }
    const quote = line.match(/^>\s?(.*)$/);
    if (quote) {
      closeList();
      output.push(`<blockquote${lineAttribute(lineNumber)} class="md-quote">${renderInline(quote[1], terms(lineNumber))}</blockquote>`);
      return;
    }
    const unordered = line.match(/^[-*+]\s+(.+)$/);
    const ordered = line.match(/^\d+\.\s+(.+)$/);
    if (unordered || ordered) {
      const target = unordered ? "ul" : "ol";
      if (list !== target) {
        closeList();
        output.push(`<${target} class="md-${target}">`);
        list = target;
      }
      output.push(`<li${lineAttribute(lineNumber)}>${renderInline((unordered ?? ordered)![1], terms(lineNumber))}</li>`);
      return;
    }
    if (/^(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      closeList();
      output.push(`<hr${lineAttribute(lineNumber)} class="md-hr" />`);
      return;
    }
    closeList();
    output.push(`<p${lineAttribute(lineNumber)} class="md-p">${renderInline(line.trim(), terms(lineNumber))}</p>`);
  });

  closeList();
  if (inCode) output.push(`<pre${lineAttribute(codeStartLine)} class="md-pre"><code class="md-block-code">${escapeHtml(code.join("\n"))}</code></pre>`);
  return output.join("\n");
}

export function highlightSnippet(snippet: string, terms: string[]): string {
  let output = escapeHtml(snippet);
  for (const term of terms) {
    const escaped = escapeHtml(term);
    if (escaped) output = output.replace(new RegExp(`(${escapeRegExp(escaped)})`, "gi"), '<mark class="hit-mark">$1</mark>');
  }
  return output;
}
