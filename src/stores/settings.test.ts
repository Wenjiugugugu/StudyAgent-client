import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";

const apiMocks = vi.hoisted(() => ({
  getSettings: vi.fn(),
  saveSettings: vi.fn(),
}));

vi.mock("@/api", () => apiMocks);

import { useSettingsStore } from "./settings";

describe("settings store persistence failures", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    apiMocks.getSettings.mockReset();
    apiMocks.saveSettings.mockReset();
  });

  it("读取失败时保留错误且不注入默认设置", async () => {
    apiMocks.getSettings.mockRejectedValue(new Error("settings.json damaged"));
    const store = useSettingsStore();

    await expect(store.load()).rejects.toThrow("settings.json damaged");
    expect(store.settings).toBeNull();
    expect(store.error).toBe("settings.json damaged");
  });

  it("读取成功时只为缺省字段补默认值", async () => {
    apiMocks.getSettings.mockResolvedValue({
      data_dir: "D:/StudyAgent/data",
      theme: "dark",
      onboarding_completed: true,
    });
    const store = useSettingsStore();

    await store.load();
    expect(store.settings?.data_directory).toBe("D:/StudyAgent/data");
    expect(store.settings?.theme).toBe("dark");
    expect(store.settings?.study_schedule.start_time).toBe("09:00");
    expect(store.error).toBeNull();
  });

  it("保存失败时向调用方传播错误", async () => {
    apiMocks.getSettings.mockResolvedValue({ data_dir: "D:/StudyAgent/data" });
    apiMocks.saveSettings.mockRejectedValue(new Error("disk full"));
    const store = useSettingsStore();
    await store.load();

    await expect(store.save()).rejects.toThrow("disk full");
  });
});
