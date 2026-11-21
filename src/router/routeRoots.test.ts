import { describe, expect, it } from "vitest";
import { parse } from "vue/compiler-sfc";

// Vue 模板 AST 的节点类型；通过 Vue 自带编译器读取，避免依赖未直接安装的内部包。
const NodeTypes = { ELEMENT: 1, TEXT: 2, COMMENT: 3 } as const;
const pages = import.meta.glob<string>(["../views/*.vue", "../features/*/*Page.vue"], {
  query: "?raw",
  import: "default",
  eager: true,
});

describe("路由页面切换", () => {
  it.each(Object.entries(pages))("%s 提供单一页面根节点", (_path, source) => {
    const { descriptor } = parse(source);
    const roots = descriptor.template!.ast!.children.filter(
      (node) =>
        node.type !== NodeTypes.COMMENT && !(node.type === NodeTypes.TEXT && !node.content.trim())
    );
    expect(roots).toHaveLength(1);
    expect(roots[0].type).toBe(NodeTypes.ELEMENT);
  });
});
