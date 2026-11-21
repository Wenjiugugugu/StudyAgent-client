/** 草稿按捕获时的日期和表单串行保存；离开页面取消后续自动保存。 */
export function createReviewDraftQueue<T>(persist: (draft: T) => Promise<void>) {
  let queue: Promise<void> = Promise.resolve();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;
  function cancel() {
    clearTimeout(timer);
    timer = undefined;
  }
  function save(draft: T) {
    if (disposed) return Promise.reject(new Error("草稿页面已离开"));
    const snapshot = structuredClone(draft);
    queue = queue.catch(() => {}).then(() => persist(snapshot));
    return queue;
  }
  function schedule(action: () => void, delay = 600) {
    cancel();
    if (!disposed) timer = setTimeout(action, delay);
  }
  function dispose() {
    cancel();
    disposed = true;
  }
  return { save, schedule, cancel, dispose, drain: () => queue };
}
