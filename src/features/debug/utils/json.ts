/**
 * 调试页 — JSON 安全格式化
 */

/**
 * 将对象格式化为缩进 JSON；使用 replacer 跟踪已序列化对象，
 * 避免循环引用抛错退化为 "[object Object]"（L50）。
 */
export function formatJson(obj: unknown): string {
  const seen = new WeakSet<object>();
  try {
    return JSON.stringify(
      obj,
      (_key, value: unknown) => {
        if (value && typeof value === "object") {
          if (seen.has(value)) {
            return "[Circular]";
          }
          seen.add(value);
        }
        return value;
      },
      2,
    );
  } catch (e) {
    return `[无法序列化: ${e instanceof Error ? e.message : String(e)}]`;
  }
}
