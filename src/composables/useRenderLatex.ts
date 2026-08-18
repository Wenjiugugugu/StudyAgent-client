import katex from "katex";

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

/**
 * 安全地渲染消息内容，保持换行并转换 LaTeX
 *
 * 占位符策略：先从原文提取所有公式段，替换为占位符，只对非公式部分做 HTML 转义，
 * 最后再将占位符还原为 KaTeX 渲染结果（避免转义破坏 LaTeX 语法，如 `$a < b$`）。
 */
export function renderMessage(content: string): string {
  // 提取所有 $...$ 与 $$...$$ 公式段（先块级再行内，与 renderLatex 顺序一致），替换为占位符
  const latexParts: string[] = [];
  const withPlaceholders = content.replace(
    /\$\$([\s\S]*?)\$\$|\$(?!\$)([\s\S]*?[^\\])\$(?!\$)/g,
    (_m, block: string | undefined, inline: string | undefined) => {
      latexParts.push(block ? `$$${block}$$` : `$${inline}$`);
      return `\u0001LATEX_${latexParts.length - 1}\u0001`;
    },
  );

  // 对非公式部分做 HTML 转义（占位符本身不含 & < >，不受影响）
  const escaped = withPlaceholders
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");

  // 还原占位符为 KaTeX 渲染结果
  const withLatex = escaped.replace(/\u0001LATEX_(\d+)\u0001/g, (_m, idx: string) => {
    const original = latexParts[parseInt(idx, 10)];
    return original === undefined ? "" : renderLatex(original);
  });

  // 处理换行
  return withLatex.replace(/\n/g, "<br>");
}
