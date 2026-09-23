import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const rootDir = dirname(dirname(fileURLToPath(import.meta.url)));
const uiDir = join(rootDir, "src", "components", "ui");
const barrelPath = join(uiDir, "index.ts");
const componentFiles = readdirSync(uiDir, { withFileTypes: true })
  .filter((entry) => entry.isFile() && entry.name.endsWith(".vue"))
  .map((entry) => entry.name)
  .sort();
const barrel = readFileSync(barrelPath, "utf8");

const exportsByFile = new Map();
const duplicateExports = [];
const invalidExports = [];
const exportPattern = /export\s+\{\s*default\s+as\s+([A-Za-z0-9_$]+)\s*\}\s+from\s+"\.\/([^"/]+\.vue)"\s*;?/g;
for (const match of barrel.matchAll(exportPattern)) {
  if (exportsByFile.has(match[2])) duplicateExports.push(match[2]);
  if (match[1] !== match[2].replace(/\.vue$/, "")) {
    invalidExports.push(`${match[1]} -> ${match[2]}`);
  }
  exportsByFile.set(match[2], match[1]);
}

const missing = componentFiles.filter((file) => !exportsByFile.has(file));
const stale = [...exportsByFile.keys()].filter((file) => !componentFiles.includes(file));

if (missing.length || stale.length || duplicateExports.length || invalidExports.length) {
  if (missing.length) {
    console.error(`UI 组件未登记到 src/components/ui/index.ts：${missing.join(", ")}`);
  }
  if (stale.length) {
    console.error(`UI 入口包含不存在的组件：${stale.join(", ")}`);
  }
  if (duplicateExports.length) {
    console.error(`UI 入口存在重复导出：${duplicateExports.join(", ")}`);
  }
  if (invalidExports.length) {
    console.error(`UI 导出名称必须与文件名一致：${invalidExports.join(", ")}`);
  }
  process.exitCode = 1;
} else {
  console.log(`UI 组件入口检查通过：${componentFiles.length} 个组件已登记。`);
}
