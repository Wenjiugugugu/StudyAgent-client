import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import * as api from "@/api";
import { useUpdateStore } from "./update";
import { registerUpdatePreparation } from "@/utils/updatePreparation";
import type { UpdateCheckResult } from "@/types";

const { prepareFocus } = vi.hoisted(() => ({ prepareFocus: vi.fn() }));
vi.mock("./focus", () => ({ useFocusStore: () => ({ prepareForUpdate: prepareFocus }) }));
vi.mock("@/api", () => ({
  checkForUpdates: vi.fn(),
  downloadUpdate: vi.fn(),
  installUpdate: vi.fn(),
  prepareUpdateTimers: vi.fn(),
  onDownloadProgress: vi.fn(),
}));

const result = (force = false): UpdateCheckResult => ({
  has_update: true,
  current_version: "0.8.1",
  latest_version: "0.9.0",
  release_name: "0.9.0",
  published_at: "",
  release_notes: "",
  message: "",
  force_update: force,
  assets: [
    {
      name: "setup.exe",
      download_url: "https://example.test/setup.exe",
      size: 10,
      kind: "inno",
      sha256: "a".repeat(64),
    },
  ],
});
let store: ReturnType<typeof useUpdateStore>;
async function settle(pending: Promise<void>) {
  let done = false;
  void pending.finally(() => {
    done = true;
  });
  await vi.waitUntil(() => done);
  await pending;
}
beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  setActivePinia(createPinia());
  vi.mocked(api.checkForUpdates).mockResolvedValue(result());
  vi.mocked(api.downloadUpdate).mockResolvedValue("verified/setup.exe");
  vi.mocked(api.onDownloadProgress).mockResolvedValue(() => {});
  store = useUpdateStore();
});
afterEach(() => {
  store.stopAutomaticChecks();
  vi.useRealTimers();
});

describe("自动下载与安全安装更新", () => {
  it("安装前完整性复核失败后重新下载，不能反复安装已失效的文件", async () => {
    await store.checkUpdate();
    vi.mocked(api.installUpdate).mockRejectedValueOnce(
      new Error("安装前完整性复核失败，安装包已删除")
    );
    await settle(store.handleInstall());
    expect(store.downloadedFilePath).toBeNull();
    await store.handleDownload();
    expect(api.downloadUpdate).toHaveBeenCalledTimes(2);
    expect(store.downloadState).toBe("downloaded");
  });
  it("普通更新自动下载，不弹窗也不自动安装；每小时复查不重复下载", async () => {
    await store.checkOnStartup();
    expect(store.downloadState).toBe("downloaded");
    expect(store.showUpdateModal).toBe(false);
    expect(api.installUpdate).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(60 * 60 * 1000);
    expect(api.checkForUpdates).toHaveBeenCalledTimes(2);
    expect(api.downloadUpdate).toHaveBeenCalledTimes(1);
  });

  it("点击安装和禁用版本自动安装都先等待草稿保存，再暂停计时", async () => {
    vi.mocked(api.checkForUpdates).mockResolvedValue(result(true));
    const order: string[] = [];
    let finishSave!: () => void;
    const unregister = registerUpdatePreparation(
      () =>
        new Promise<void>((resolve) => {
          finishSave = () => {
            order.push("draft");
            resolve();
          };
        })
    );
    prepareFocus.mockImplementation(async () => {
      order.push("focus");
    });
    vi.mocked(api.prepareUpdateTimers).mockImplementation(async () => {
      order.push("tasks");
    });
    vi.mocked(api.installUpdate).mockImplementation(async () => {
      order.push("install");
    });
    try {
      const checking = store.checkOnStartup();
      await vi.waitUntil(() => store.preparing);
      expect(store.showUpdateModal).toBe(true);
      expect(api.installUpdate).not.toHaveBeenCalled();
      finishSave();
      await settle(checking);
      expect(order).toEqual(["draft", "focus", "tasks", "install"]);
      await store.handleInstall();
      expect(api.installUpdate).toHaveBeenCalledTimes(1);
    } finally {
      unregister();
    }
  });

  it("草稿保存失败阻止安装，重试时复用已校验包", async () => {
    await store.checkUpdate();
    const unregister = registerUpdatePreparation(async () => {
      throw new Error("磁盘已满");
    });
    await settle(store.handleInstall());
    expect(store.downloadError).toContain("磁盘已满");
    expect(store.downloadState).toBe("error");
    expect(api.installUpdate).not.toHaveBeenCalled();
    unregister();
    await settle(store.handleDownload());
    expect(api.downloadUpdate).toHaveBeenCalledTimes(1);
    expect(api.installUpdate).toHaveBeenCalledTimes(1);
  });

  it("下载失败不立即循环重试；下一次定时检查才重新下载", async () => {
    vi.mocked(api.downloadUpdate).mockRejectedValueOnce(new Error("断网"));
    await store.checkOnStartup();
    expect(store.downloadState).toBe("error");
    expect(api.downloadUpdate).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(60 * 60 * 1000);
    expect(api.downloadUpdate).toHaveBeenCalledTimes(2);
    expect(store.downloadState).toBe("downloaded");
  });

  it("重复检查和下载按钮不会启动第二个下载", async () => {
    let finish!: (path: string) => void;
    vi.mocked(api.downloadUpdate).mockReturnValueOnce(
      new Promise((resolve) => {
        finish = resolve;
      })
    );
    const checking = store.checkUpdate();
    await vi.waitUntil(() => store.downloadState === "downloading");
    await store.checkUpdate();
    await store.handleDownload();
    expect(api.downloadUpdate).toHaveBeenCalledTimes(1);
    finish("verified/setup.exe");
    await checking;
  });

  it("离线复查不会丢失已下载包或撤销已确认的禁用状态", async () => {
    vi.mocked(api.checkForUpdates).mockResolvedValue(result(true));
    prepareFocus.mockRejectedValueOnce(new Error("计时保存失败"));
    await settle(store.checkUpdate());
    vi.mocked(api.checkForUpdates).mockResolvedValue({
      ...result(),
      has_update: false,
      latest_version: "",
      message: "网络不可用",
    });
    await store.checkUpdate();
    expect(store.forceUpdate).toBe(true);
    expect(store.downloadedFilePath).toBe("verified/setup.exe");
    expect(api.installUpdate).not.toHaveBeenCalled();
  });

  it("校验失败不会得到可安装状态", async () => {
    vi.mocked(api.downloadUpdate).mockRejectedValue(new Error("SHA-256 不匹配"));
    await store.checkUpdate();
    expect(store.downloadedFilePath).toBeNull();
    expect(store.downloadError).toContain("SHA-256");
    expect(api.installUpdate).not.toHaveBeenCalled();
  });
});
