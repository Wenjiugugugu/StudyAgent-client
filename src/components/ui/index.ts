/**
 * 基础 UI 组件统一入口。
 *
 * 新功能优先从这里查找并引入组件，避免因为组件路径分散而重复实现按钮、
 * 卡片、表单控件、状态提示和弹窗等基础交互。
 */
export { default as Badge } from "./Badge.vue";
export { default as Button } from "./Button.vue";
export { default as Card } from "./Card.vue";
export { default as Checkbox } from "./Checkbox.vue";
export { default as DatePicker } from "./DatePicker.vue";
export { default as EmptyState } from "./EmptyState.vue";
export { default as LoadingSpinner } from "./LoadingSpinner.vue";
export { default as Modal } from "./Modal.vue";
export { default as ProgressBar } from "./ProgressBar.vue";
export { default as Select } from "./Select.vue";
export { default as TimePicker } from "./TimePicker.vue";

export { isTopModal, registerModal, unregisterModal, type ModalStackEntry } from "./modal-stack";
