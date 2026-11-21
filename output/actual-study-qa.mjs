import { chromium } from 'file:///C:/Users/Administrator/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright/index.mjs';
import assert from 'node:assert/strict';
import { createServer } from '../node_modules/vite/dist/node/index.js';
const server = await createServer({ server: {port:1431, host:'localhost', watch:{ignored:['**/src-tauri/target/**']}} });
await server.listen();
const browser = await chromium.launch({headless:true, executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe'});
const page = await browser.newPage({viewport:{width:1100,height:1000}});
try {
const errors = [];
page.on('pageerror', error => errors.push(error.message));
page.on('console', message => { if(message.type() === 'error') console.log('页面错误:', message.text()); });
await page.goto('http://localhost:1431/output/actual-study-qa.html');
await page.getByRole('heading', {name:'当前学习方向'}).waitFor().catch(async error => {
  console.log('诊断:', errors, await page.locator('body').innerText());
  await page.screenshot({path:'output/actual-study-qa-error.png',fullPage:true});
  await browser.close();
  throw error;
});
await page.getByRole('button', {name:'实际学习内容与计划不同？告诉 AI'}).click();
await page.getByPlaceholder('例如：我已经开始线代强化，正在学行列式。接下来想先完成线代强化，再继续高数强化。').fill('今天临时学了矩阵，但后续继续原方向');
await page.getByRole('button', {name:'仅记录当天', exact:true}).click();
let entries = JSON.parse(await page.locator('#entries').textContent());
assert.equal(entries[0].scope, 'today');
await page.getByRole('button', {name:'接下来按这个方向安排', exact:true}).click();
entries = JSON.parse(await page.locator('#entries').textContent());
assert.equal(entries[0].scope, 'ongoing');
await page.getByRole('button', {name:'补充其他科目'}).click();
entries = JSON.parse(await page.locator('#entries').textContent());
assert.equal(entries[1].subject, 'english');
assert.ok(await page.getByRole('alert').filter({hasText:'请填写'}).isVisible());
await page.getByRole('button', {name:'移除', exact:true}).nth(1).click();
await page.screenshot({path:'output/actual-study-desktop.png', fullPage:true});
await page.getByRole('button', {name:'修改', exact:true}).click();
await page.locator('.direction-panel textarea').fill('先学线代矩阵强化');
await page.getByRole('button', {name:'保存方向'}).click();
await page.locator('.direction-content').filter({hasText:'先学线代矩阵强化'}).waitFor();
let calls = await page.evaluate(() => window.qaCalls);
assert.equal(calls.find(call => call.command === 'update_study_direction').args.expectedUpdatedAt, 'v1');
await page.getByRole('button', {name:'结束方向'}).click();
await page.getByRole('status').filter({hasText:'已结束'}).waitFor();
assert.equal(await page.getByRole('button', {name:'修改',exact:true}).count(), 0);
await page.setViewportSize({width:420,height:980});
await page.screenshot({path:'output/actual-study-mobile.png', fullPage:true});
assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
assert.deepEqual(errors, []);
console.log('界面验证通过：输入、范围切换、多科目校验、修改、结束、窄屏无横向溢出。');
await browser.close();
} catch(error) {
  console.log('界面诊断:', await page.locator('body').innerText().catch(()=>'已关闭'), await page.evaluate(()=>window.qaCalls).catch(()=>[]));
  await page.screenshot({path:'output/actual-study-qa-error.png', fullPage:true}).catch(()=>{});
  throw error;
} finally {
  await browser.close();
  await server.close();
}
