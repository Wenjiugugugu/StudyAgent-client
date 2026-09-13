import katex from "katex";
import { marked } from "marked";
import DOMPurify from "dompurify";

/**
 * 将包含 LaTeX 公式的文本渲染为 HTML
 * 支持 $...$ (行内公式) 和 $$...$$ (块级公式)
 */
export function renderLatex(text: string): string {
  if (!text) return text;

  // 先处理块级公式 $$...$$
  let result = text.replace(/\$\$([\s\S]*?)\$\$/g, (_match, formula: string) => {
    try {
      const html = katex.renderToString(formula.trim(), {
        throwOnError: false,
        displayMode: true,
      });
      return `<div class="latex-block">${html}</div>`;
    } catch {
      return `<code>${formula}</code>`;
    }
  });

  // 再处理行内公式 $...$（不在已处理的块级公式内）
  result = result.replace(/\$(?!\$)([\s\S]*?[^\\])\$(?!\$)/g, (_match, formula: string) => {
    try {
      const html = katex.renderToString(formula.trim(), {
        throwOnError: false,
        displayMode: false,
      });
      return `<span class="latex-inline">${html}</span>`;
    } catch {
      return `<code>${formula}</code>`;
    }
  });

  return result;
}

// ── marked 配置 ──
// GFM：表格、删除线、任务列表、autolink
// breaks：单换行也转 <br>（聊天场景更自然，对齐原 renderMessage 行为）
marked.setOptions({
  gfm: true,
  breaks: true,
});

/**
 * 解析 Markdown 为 HTML 字符串（同步）。
 *
 * marked v18 默认可能返回 string | Promise<string>，这里强制同步：
 * 通过 `async: false` 选项确保返回 string。
 */
function parseMarkdown(text: string): string {
  return marked.parse(text, { async: false }) as string;
}

/**
 * 安全地渲染消息内容：同时支持 Markdown 结构与 LaTeX 公式。
 *
 * 渲染流水线（占位符策略保护 LaTeX 不被 Markdown / 转义破坏）：
 * 1. 从原文提取所有 $$...$$ 与 $...$ 公式段，替换为 \u0001KTEX_N\u0001 占位符
 *    （控制字符 \u0001 不会被 AI 输出，碰撞概率为零；marked 不会触碰它）
 * 2. 用 marked 解析占位符后的文本 → Markdown HTML
 * 3. 用 DOMPurify 过滤 XSS（允许 KaTeX 输出需要的 MathML 标签与属性）
 * 4. 还原占位符为 KaTeX 渲染结果：
 *    - 块级公式占位符若被 <p> 包裹（典型：独占一行），整体替换为 KaTeX 块并去掉外层 <p>
 *      （<p> 内不能嵌 <div>，否则是无效 HTML）
 *    - 内联公式占位符直接替换为 KaTeX 内联 <span>
 *
 * 兼容性：对不含任何 Markdown 语法的纯文本，输出与旧版一致（breaks 已开，
 * 单换行仍渲染为 <br>）；旧调用方 DoubtView 无需改动即可获得 Markdown 能力。
 */
export function renderMessage(content: string): string {
  if (!content) return "";

  // 1. 提取公式段为占位符（顺序：先块级 $$...$$，再行内 $...$，与 renderLatex 一致）
  const latexParts: { type: "block" | "inline"; raw: string }[] = [];
  const withPlaceholders = content.replace(
    /\$\$([\s\S]*?)\$\$|\$(?!\$)([\s\S]*?[^\\])\$(?!\$)/g,
    (_m, block: string | undefined, inline: string | undefined) => {
      latexParts.push(
        block !== undefined
          ? { type: "block", raw: `$$${block}$$` }
          : { type: "inline", raw: `$${inline}$` },
      );
      return `\u0001KTEX${latexParts.length - 1}\u0001`;
    },
  );

  // 2. marked 解析 Markdown
  const mdHtml = parseMarkdown(withPlaceholders);

  // 3. DOMPurify 过滤
  //    允许 KaTeX 输出用到的 MathML 标签与 class/style/aria-* 等属性。
  //    占位符 \u0001KTEX_N\u0001 是纯文本，安全通过过滤。
  const clean = DOMPurify.sanitize(mdHtml, {
    // KaTeX MathML 输出涉及的标签（displayMode + htmlAndMathml 默认输出）
    ADD_TAGS: [
      "math", "semantics", "annotation", "annotation-xml",
      "mrow", "mi", "mo", "mn", "ms", "mtext", "mspace", "mstyle",
      "msup", "msub", "msubsup", "mfrac", "msqrt", "mroot",
      "mtable", "mtr", "mtd", "maligngroup", "malignmark",
      "menclose", "merror", "mpadded", "mfenced", "mlongdiv", "mscarries", "msline", "msgroup", "msrow", "mstack",
    ],
    ADD_ATTR: [
      // 通用属性
      "class", "style", "id", "title", "lang", "dir",
      // 链接/媒体
      "href", "src", "alt", "target", "rel", "width", "height",
      // 表格
      "colspan", "rowspan", "align", "valign", "span",
      // KaTeX / ARIA
      "aria-hidden", "role",
      // KaTeX MathML annotation 的 encoding 属性
      "encoding",
    ],
    // 不剥离 data-* 属性（部分库可能用 data 属性做标记）
    ALLOW_DATA_ATTR: true,
    // 链接强制加 rel="noopener noreferrer" target="_blank" 由样式/行为层处理
    // 这里保持默认安全：禁止 <script>/<iframe> 等
  });

  // 4. 还原占位符为 KaTeX 渲染结果
  //    4a. 块级公式：若占位符被 <p> 独占（独占一行场景），整体替换并去掉外层 <p>
  //        <p>\u0001KTEX_N\u0001</p> → <div class="latex-block">...</div>
  //        （允许 p 标签前后有空白）
  const withBlocksRestored = clean.replace(
    /<p>\s*\u0001KTEX(\d+)\u0001\s*<\/p>/g,
    (_m: string, idx: string) => {
      const part = latexParts[parseInt(idx, 10)];
      // 仅块级公式走这条路径；内联公式占位符若意外被 <p> 独占，也走内联渲染
      return part ? renderLatex(part.raw) : "";
    },
  );

  //    4b. 其余占位符（内联，或块级公式落在列表项/表格单元里未被 <p> 独占）直接替换
  const withKatex = withBlocksRestored.replace(/\u0001KTEX(\d+)\u0001/g, (_m: string, idx: string) => {
    const part = latexParts[parseInt(idx, 10)];
    return part ? renderLatex(part.raw) : "";
  });

  return withKatex;
}
