import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import assert from "node:assert/strict";
const root=path.resolve(import.meta.dirname,"../..");
const files=["src-tauri/src/ai/provider.rs","src-tauri/src/ai/task_prompts.rs","src-tauri/src/core/planner.rs","src-tauri/src/core/planning/prompts.rs","src-tauri/src/core/planning/parsing.rs","src-tauri/src/core/planning/constraints.rs","src-tauri/src/core/goal_planner.rs","src-tauri/src/core/review.rs","src-tauri/src/api/commands/progress_commands.rs"];
const read=(file)=>fs.readFileSync(path.join(root,file),"utf8");
const planner=read(files[2]),prompts=read(files[3]),contracts=read(files[1]),progress=read(files[8]);
const observations={
  directionValidatorHasOwnContract:planner.includes("task_prompts::CHECK"),
  regenerationHasDaysContract:planner.includes("task_prompts::DAYS"),
  estimationHasArrayContract:read(files[6]).includes("task_prompts::ESTIMATE"),
  reviewOnlyGeneratesView:read(files[7]).includes("task_prompts::REVIEW") && read(files[7]).includes("factual_review"),
  syntaxRepairIsVerified:planner.includes("verify_repair") && read(files[4]).includes("baseline != &actual"),
  calendarUsesAvailableDates:prompts.includes("constraints::available_dates") && !prompts.includes("study_days.saturating_sub(excluded_days.len())"),
  remainingBudgetIsScaledByProgram:planner.includes("scale_remaining_budget"),
  pendingIsQuoted:progress.includes("pending") && progress.includes("带双引号的 JSON 字符串"),
  noWholeSyllabusForcedIntoOneWeek:!prompts.includes("每个待学章节/知识点都必须被本周任务覆盖推进"),
  noFalseRetrievalClaim:contracts.includes("本次请求没有联网检索工具") && !prompts.includes("请联网检索"),
};
for(const [name,passed] of Object.entries(observations))assert(passed,name);
const skills=["kaoyan-plan","kaoyan-review","kaoyan-status","kaoyan-teach"].map(name=>{
  const archive=`.codex/skills/${name}/SKILL.deprecated.md`;
  const disabled=!fs.existsSync(path.join(root,`.codex/skills/${name}/SKILL.md`))&&fs.existsSync(path.join(root,archive));
  assert(disabled,name);return {name,disabled,archive};
});
const evidence={scope:"整改后的静态契约核对，不代表真实模型质量评估；回归行为由正式测试验证",observations,skills,files:files.map(file=>({file,sha256:crypto.createHash("sha256").update(fs.readFileSync(path.join(root,file))).digest("hex")}))};
fs.writeFileSync(path.join(import.meta.dirname,"prompt-source-evidence.json"),JSON.stringify(evidence,null,2));
console.log(JSON.stringify({observations,skills},null,2));
