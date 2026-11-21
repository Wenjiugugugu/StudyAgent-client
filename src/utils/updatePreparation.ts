import { nextTick, ref, watch } from "vue";

export const taskTimerPauseVersion = ref(0);
export const updatePreparationActive = ref(false);
export function assertUserMutationAllowed() {
  if (updatePreparationActive.value)
    throw new Error("正在准备更新，学习记录已暂停编辑。更新失败后可以继续。");
}

const requests = new Set<Promise<unknown>>();
const preparations = new Set<() => Promise<void>>();

/** 等待整个桌面请求，包括请求返回后串接的保存步骤。 */
export async function trackDesktopRequest<T>(request: Promise<T>): Promise<T> {
  requests.add(request);
  try {
    return await request;
  } finally {
    requests.delete(request);
  }
}

export function registerUpdatePreparation(prepare: () => Promise<void>) {
  preparations.add(prepare);
  return () => preparations.delete(prepare);
}

export async function waitForIdle(busy: () => boolean) {
  if (!busy()) return;
  await new Promise<void>((resolve) => {
    const stop = watch(busy, (value) => {
      if (!value) {
        stop();
        resolve();
      }
    });
  });
}

export async function prepareViewsForUpdate() {
  await Promise.all([...preparations].map((prepare) => prepare()));
  do {
    if (requests.size) await Promise.allSettled([...requests]);
    await nextTick();
    // 跨到下一轮事件循环，让调用方串接的保存及界面状态收尾完成。
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
  } while (requests.size);
}
