import { afterEach, describe, expect, it, vi } from "vitest";

const tauriInvoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriInvoke,
}));

import { invokeWithFallback } from "./tauri";

function setTauriEnvironment(enabled: boolean) {
  if (enabled) {
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: { __TAURI_INTERNALS__: {} },
    });
  } else {
    Reflect.deleteProperty(globalThis, "window");
  }
}

afterEach(() => {
  setTauriEnvironment(false);
  tauriInvoke.mockReset();
});

describe("invokeWithFallback", () => {
  it("在浏览器开发模式使用 mock 数据", async () => {
    setTauriEnvironment(false);
    const fallback = vi.fn().mockResolvedValue("mock");

    await expect(invokeWithFallback("read_state", undefined, fallback)).resolves.toBe("mock");
    expect(fallback).toHaveBeenCalledOnce();
    expect(tauriInvoke).not.toHaveBeenCalled();
  });

  it("在桌面环境透传后端错误，不制造假成功", async () => {
    setTauriEnvironment(true);
    const backendError = new Error("disk full");
    tauriInvoke.mockRejectedValue(backendError);
    const fallback = vi.fn().mockResolvedValue("mock");

    await expect(invokeWithFallback("save_settings", {}, fallback)).rejects.toBe(backendError);
    expect(fallback).not.toHaveBeenCalled();
  });

  it("在桌面环境返回真实后端结果", async () => {
    setTauriEnvironment(true);
    tauriInvoke.mockResolvedValue({ ok: true });
    const fallback = vi.fn().mockResolvedValue({ ok: false });

    await expect(invokeWithFallback("get_state", undefined, fallback)).resolves.toEqual({
      ok: true,
    });
    expect(fallback).not.toHaveBeenCalled();
  });
});
