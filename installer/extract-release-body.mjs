/**
 * 从 src/changelogs/index.ts 提取指定版本的「发布页说明」到 installer/release-<ver>-body.md
 *
 * 用法：node installer/extract-release-body.mjs 0.7.2
 * 说明：release body 取 `VERSION_RELEASE_NOTES`（详细版，面向下载/查阅者）；
 *       应用内弹窗文案是 `VERSION_CHANGELOGS`（简短版，一行一条），两者不要混用。
 */
import fs from "node:fs";
import path from "node:path";

const ver = process.argv[2];
if (!ver) {
  console.error("用法: node installer/extract-release-body.mjs <version>");
  process.exit(1);
}

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1")), "..");
const srcPath = path.join(root, "src", "changelogs", "index.ts");
const src = fs.readFileSync(srcPath, "utf8");

/** 取出 `NAME: Record<...> = { ... }` 中某个 key 的字符串数组内容 */
function extract(exportName, version) {
  const start = src.indexOf(`export const ${exportName}`);
  if (start < 0) return null;
  const objStart = src.indexOf("{", src.indexOf("=", start));
  // 逐字符扫描匹配括号，限定在该顶层对象内查找版本键
  let depth = 0;
  let objEnd = -1;
  for (let i = objStart; i < src.length; i++) {
    const c = src[i];
    if (c === "{") depth++;
    else if (c === "}") {
      depth--;
      if (depth === 0) {
        objEnd = i;
        break;
      }
    }
  }
  const scope = src.slice(objStart, objEnd);
  const keyIdx = scope.indexOf(`"${version}":`);
  if (keyIdx < 0) return null;
  const arrStart = scope.indexOf("[", keyIdx);
  depth = 0;
  let arrEnd = -1;
  for (let i = arrStart; i < scope.length; i++) {
    const c = scope[i];
    if (c === "[") depth++;
    else if (c === "]") {
      depth--;
      if (depth === 0) {
        arrEnd = i;
        break;
      }
    }
  }
  const items = [];
  for (const line of scope.slice(arrStart + 1, arrEnd).split("\n")) {
    const m = line.match(/^\s*("(?:[^"\\]|\\.)*")\s*,?\s*$/);
    if (m) items.push(JSON.parse(m[1]));
  }
  return items;
}

const items = extract("VERSION_RELEASE_NOTES", ver);
if (!items || items.length === 0) {
  console.error(`未找到 ${ver} 的发布说明（VERSION_RELEASE_NOTES 中缺失？）`);
  process.exit(1);
}
const out = path.join(root, "installer", `release-${ver}-body.md`);
fs.writeFileSync(out, items.join("\n") + "\n", "utf8");
console.log(`wrote ${path.relative(root, out)} (${items.length} lines)`);
