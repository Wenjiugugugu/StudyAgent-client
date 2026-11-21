// 旧的一次性探针已迁入正式回归：直接测试真实 Pinia store 的请求乱序。
import { spawnSync } from "node:child_process";
import path from "node:path";
const root=path.resolve(import.meta.dirname,"../..");
const result=spawnSync(process.execPath,[path.join(root,"node_modules/vitest/vitest.mjs"),"run","src/stores/analytics.test.ts"],{cwd:root,stdio:"inherit"});
if(result.error)throw result.error;
process.exitCode=result.status??1;
