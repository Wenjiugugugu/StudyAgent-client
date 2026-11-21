import { expect, it } from "vitest";
import {
  prepareViewsForUpdate,
  trackDesktopRequest,
  updatePreparationActive,
  assertUserMutationAllowed,
} from "./updatePreparation";

it("退出前等待异步请求之后串接的第二次保存", async () => {
  let first!: () => void, second!: () => void;
  const order: string[] = [];
  const pending = trackDesktopRequest(
    new Promise<void>((resolve) => {
      first = resolve;
    })
  )
    .then(() =>
      trackDesktopRequest(
        new Promise<void>((resolve) => {
          second = resolve;
        })
      )
    )
    .then(() => {
      order.push("saved");
    });
  const preparation = prepareViewsForUpdate().then(() => {
    order.push("prepared");
  });
  first();
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  second();
  await Promise.all([pending, preparation]);
  expect(order).toEqual(["saved", "prepared"]);
});

it("准备更新期间拒绝新编辑，失败恢复后允许继续", () => {
  updatePreparationActive.value = true;
  expect(() => assertUserMutationAllowed()).toThrow("暂停编辑");
  updatePreparationActive.value = false;
  expect(() => assertUserMutationAllowed()).not.toThrow();
});
