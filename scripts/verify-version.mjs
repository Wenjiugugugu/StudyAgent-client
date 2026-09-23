import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function readJson(relativePath) {
  return JSON.parse(readFileSync(resolve(root, relativePath), "utf8"));
}

function readVersionFromCargo() {
  const cargo = readFileSync(resolve(root, "src-tauri/Cargo.toml"), "utf8");
  const match = cargo.match(/^version\s*=\s*"([^"]+)"/m);
  if (!match) throw new Error("src-tauri/Cargo.toml 中缺少 package.version");
  return match[1];
}

function readVersionFromFrontendFallback() {
  const source = readFileSync(resolve(root, "src/version.ts"), "utf8");
  const match = source.match(/FALLBACK_APP_VERSION\s*=\s*"([^"]+)"/);
  if (!match) throw new Error("src/version.ts 中缺少 FALLBACK_APP_VERSION");
  return match[1];
}

const versions = {
  "package.json": readJson("package.json").version,
  "src-tauri/Cargo.toml": readVersionFromCargo(),
  "src-tauri/tauri.conf.json": readJson("src-tauri/tauri.conf.json").version,
  "src/version.ts": readVersionFromFrontendFallback(),
};

const uniqueVersions = new Set(Object.values(versions));
if (uniqueVersions.size !== 1 || [...uniqueVersions].some((version) => !version)) {
  console.error("版本号不一致：");
  for (const [source, version] of Object.entries(versions)) {
    console.error(`- ${source}: ${version || "<missing>"}`);
  }
  process.exitCode = 1;
} else {
  console.log(`版本号一致：${Object.values(versions)[0]}`);
}
