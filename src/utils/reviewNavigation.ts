type Direction = "prev" | "next" | "jump";
/** 切换日期前必须保存当前草稿；失败保留日期和编辑内容。 */
export function createReviewNavigation(options: {
  currentDate: () => string;
  busy: () => boolean;
  saveDraft: () => Promise<void>;
  commit: (date: string, direction: Direction) => void;
  onError: (error: unknown) => void;
}) {
  let changing = false;
  return async (date: string, direction: Direction = "jump") => {
    if (date === options.currentDate() || changing || options.busy()) return false;
    changing = true;
    try {
      await options.saveDraft();
      options.commit(date, direction);
      return true;
    } catch (error) {
      options.onError(error);
      return false;
    } finally {
      changing = false;
    }
  };
}
