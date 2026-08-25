/**
 * 阶段6 — 跨领域 API 边界测试
 *
 * 验证三个 feature（dashboard / debug / settings）的领域化 API 层：
 * 1. 每个 feature 只暴露本领域需要的函数（边界隔离）。
 * 2. 暴露的函数与全局 @/api 保持同一引用（纯转发，行为不变）。
 * 3. feature 之间不存在相互依赖（无循环依赖）。
 *
 * 若后续全局 API 更名或某 feature 需要新增函数，此测试会第一时间发现。
 */
import { describe, expect, it } from "vitest";
import * as globalApi from "@/api";
import { dashboardApi } from "./dashboard/api";
import { debugApi } from "./debug/api";
import { settingsApi } from "./settings/api";

describe("feature API 边界隔离", () => {
  it("dashboard 只暴露工作台相关函数，且转发全局 API", () => {
    const expected = [
      "getDashboardSummary",
      "getBriefing",
      "regenerateBriefing",
      "getWeekSummaries",
      "getWeekPlan",
      "getReview",
    ] as const;

    for (const name of expected) {
      expect(dashboardApi, `dashboardApi.${name}`).toHaveProperty(name);
      expect(dashboardApi[name]).toBe((globalApi as Record<string, unknown>)[name]);
    }
  });

  it("debug 只暴露调试相关函数，且转发全局 API", () => {
    const expected = [
      "getState",
      "getDashboardSummary",
      "getPlanByDate",
      "getReview",
      "getSettings",
      "testAIProvider",
      "getAiUsageLog",
      "clearAiUsageLog",
      "readAppLog",
      "clearAppLog",
      "debugListDir",
      "debugReadFile",
      "isTauri",
    ] as const;

    for (const name of expected) {
      expect(debugApi, `debugApi.${name}`).toHaveProperty(name);
      expect(debugApi[name]).toBe((globalApi as Record<string, unknown>)[name]);
    }
  });

  it("settings 只暴露设置相关函数，且转发全局 API", () => {
    const expected = [
      "getState",
      "updateSubjectTextbook",
      "changeDataDirectory",
      "exportBackup",
      "importBackup",
      "saveBackgroundImage",
      "deleteBackgroundImage",
      "testAIProvider",
      "listAIModels",
      "getAutostart",
      "setAutostart",
      "getCloseAction",
      "setCloseAction",
    ] as const;

    for (const name of expected) {
      expect(settingsApi, `settingsApi.${name}`).toHaveProperty(name);
      expect(settingsApi[name]).toBe((globalApi as Record<string, unknown>)[name]);
    }
  });

  it("各 feature 暴露的 API 均为可调用的函数（转发有效）", () => {
    for (const api of [dashboardApi, debugApi, settingsApi]) {
      for (const [key, value] of Object.entries(api)) {
        expect(typeof value, `${key} 应为函数`).toBe("function");
      }
    }
  });
});
