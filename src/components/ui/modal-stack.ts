/**
 * 全局模态框栈
 *
 * 多个 Modal 同时打开（弹窗叠弹窗）时，每个实例都会注册 document keydown 监听，
 * 若各自响应 ESC 会导致一次关闭全部。本模块维护一个全局栈：
 * - ESC 只交给最顶层处于打开状态的模态处理；
 * - 顶层模态不可用 ESC 关闭时，不向下层穿透。
 */
export type ModalStackEntry = {
  key: number;
  isOpen: () => boolean;
  closeOnEsc: () => boolean;
  close: () => void;
};

let nextKey = 1;
const stack: ModalStackEntry[] = [];

/** 注册一个模态框，返回唯一 key；组件卸载时须调用 unregisterModal 移除 */
export function registerModal(entry: Omit<ModalStackEntry, "key">): number {
  const key = nextKey++;
  stack.push({ ...entry, key });
  return key;
}

export function unregisterModal(key: number): void {
  const i = stack.findIndex((e) => e.key === key);
  if (i >= 0) stack.splice(i, 1);
}

/** 当前实例是否为最顶层且处于打开状态的模态框 */
export function isTopModal(key: number): boolean {
  for (let i = stack.length - 1; i >= 0; i--) {
    if (stack[i].isOpen()) return stack[i].key === key;
  }
  return false;
}
