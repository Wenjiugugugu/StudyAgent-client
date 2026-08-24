/**
 * 工作台日期展示辅助（纯函数）
 */

/** 今日长日期（上海时区，含星期），如「2026年8月25日 星期二」 */
export function dateLabelShanghai(): string {
  const d = new Date();
  return new Intl.DateTimeFormat("zh-CN", {
    timeZone: "Asia/Shanghai",
    year: "numeric",
    month: "long",
    day: "numeric",
    weekday: "long",
  }).format(d);
}

const WEEKDAYS_SHORT = ["日", "一", "二", "三", "四", "五", "六"];

/** 日期字符串 → 单字星期（日/一/二/三/四/五/六） */
export function weekdayShort(dateStr: string): string {
  const d = new Date(dateStr);
  return WEEKDAYS_SHORT[d.getDay()] ?? "";
}

/** 是否为今天 */
export function isToday(dateStr: string, todayDateStr: string): boolean {
  return dateStr === todayDateStr;
}
