import { describe, expect, it } from "vitest";
import { escapeHtml, highlightSnippet, renderMarkdown } from "./markdown";

describe("safe markdown rendering", () => {
  it("escapes HTML and attribute-breaking characters", () => {
    expect(escapeHtml(`<img src=x onerror="alert(1)">'`)).toBe("&lt;img src=x onerror=&quot;alert(1)&quot;&gt;&#39;");
    expect(renderMarkdown(`<script>alert('x')</script>`)).not.toContain("<script>");
  });

  it("rejects executable and malformed links", () => {
    const html = renderMarkdown(`[bad](javascript:alert(1)) [data](data:text/html,x) [quote](https://example.com/\" onmouseover=\"x)`);
    expect(html).not.toContain("javascript:");
    expect(html).not.toContain("data:text");
    expect(html).not.toContain("onmouseover=");
  });

  it("keeps approved links and safely highlights snippets", () => {
    expect(renderMarkdown(`[docs](https://example.com/a?q=1)`)).toContain('rel="noopener noreferrer"');
    expect(highlightSnippet(`<b>term</b>`, ["term"])).toBe("&lt;b&gt;<mark class=\"hit-mark\">term</mark>&lt;/b&gt;");
  });
});
