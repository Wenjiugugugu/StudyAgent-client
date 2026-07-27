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
 */
export function renderMessage(content: string): string {
  // 对非公式部分做 HTML 转义，然后渲染公式
  const escaped = content
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");

  // 渲染 LaTeX
  const withLatex = renderLatex(escaped);

  // 处理换行
  return withLatex.replace(/\n/g, "<br>");
}
