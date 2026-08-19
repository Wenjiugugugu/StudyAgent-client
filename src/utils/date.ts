/**
 * 日期工具 —— 统一使用 Asia/Shanghai（UTC+8）时区，避免在 00:00-08:00 期间
 * 因 UTC 偏移而得到前一天的日期。
 */

/**
 * 获取当前日期字符串（YYYY-MM-DD，上海时区）
 */
export function todayString(): string {
  return formatDateShanghai(new Date());
}

/**
 * 获取昨天的日期字符串（YYYY-MM-DD，上海时区）
 */
export function yesterdayString(): string {
  const d = new Date();
  d.setDate(d.getDate() - 1);
  return formatDateShanghai(d);
}

/**
 * 计算指定日期减去一天后的日期字符串（YYYY-MM-DD）
 */
export function prevDateString(dateStr: string): string {
  const [y, m, d] = dateStr.split("-").map(Number);
  const dt = new Date(y, m - 1, d, 12, 0, 0);
  dt.setDate(dt.getDate() - 1);
  const yy = dt.getFullYear();
  const mm = String(dt.getMonth() + 1).padStart(2, "0");
  const dd = String(dt.getDate()).padStart(2, "0");
  return `${yy}-${mm}-${dd}`;
}

/**
 * 计算指定日期加上一天后的日期字符串（YYYY-MM-DD）
 */
export function nextDateString(dateStr: string): string {
  const [y, m, d] = dateStr.split("-").map(Number);
  const dt = new Date(y, m - 1, d, 12, 0, 0);
  dt.setDate(dt.getDate() + 1);
  const yy = dt.getFullYear();
  const mm = String(dt.getMonth() + 1).padStart(2, "0");
  const dd = String(dt.getDate()).padStart(2, "0");
  return `${yy}-${mm}-${dd}`;
}

/**
 * 将 Date 对象格式化为 YYYY-MM-DD（上海时区）
 */
export function formatDateShanghai(d: Date): string {
  const parts = new Intl.DateTimeFormat("zh-CN", {
    timeZone: "Asia/Shanghai",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).formatToParts(d);

  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? "";
  return `${get("year")}-${get("month")}-${get("day")}`;
}

/**
 * 获取当前时间字符串（YYYY-MM-DDTHH:mm，上海时区）
 */
export function nowString(): string {
  const d = new Date();
  const date = formatDateShanghai(d);
  const parts = new Intl.DateTimeFormat("zh-CN", {
    timeZone: "Asia/Shanghai",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).formatToParts(d);
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? "";
  return `${date}T${get("hour")}:${get("minute")}`;
}

/**
 * 将 Date 格式化为上海时区的 ISO 时间（YYYY-MM-DDTHH:mm:ss+08:00）
 *
 * 供专注会话等按"本地日期"落盘的数据使用：后端按字符串前 10 位取日期，
 * 若用 UTC 的 toISOString()，在 00:00-08:00 之间会落到前一天，导致
 * 按本地日期读取时查不到记录。
 */
export function formatLocalIso(d: Date): string {
  const date = formatDateShanghai(d);
  const parts = new Intl.DateTimeFormat("zh-CN", {
    timeZone: "Asia/Shanghai",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).formatToParts(d);
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? "";
  // Intl 在午夜可能返回 "24"，归一为 "00"
  const hour = get("hour") === "24" ? "00" : get("hour");
  return `${date}T${hour}:${get("minute")}:${get("second")}+08:00`;
}

/**
 * 获取日期的中文星期几名称（周日 至 周六）
 * M38：从 YYYY-MM-DD 分量直接构造 Date（本地时区无关），避免中午 hack 与文件头时区声明不一致
 */
export function weekdayName(dateStr: string): string {
  const [y, m, d] = dateStr.split("-").map(Number);
  if (!y || !m || !d) return "";
  const dt = new Date(y, m - 1, d, 12, 0, 0);
  const weekdays = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
  return weekdays[dt.getDay()] ?? "";
}

/**
 * 计算两个 YYYY-MM-DD 日期之间的天数差（endDate - startDate）。
 * 结果忽略时间部分，仅按日期计算；若格式无效则返回 0。
 */
export function daysBetween(endDate: string, startDate: string): number {
  const parse = (s: string) => {
    const [y, m, d] = s.split("-").map(Number);
    return new Date(y, m - 1, d);
  };
  const end = parse(endDate);
  const start = parse(startDate);
  if (isNaN(end.getTime()) || isNaN(start.getTime())) {
    return 0;
  }
  end.setHours(0, 0, 0, 0);
  start.setHours(0, 0, 0, 0);
  const diffTime = end.getTime() - start.getTime();
  const diffDays = Math.ceil(diffTime / (1000 * 60 * 60 * 24));
  return Math.max(0, diffDays);
}

/**
 * 获取指定日期所在周的周一日期（YYYY-MM-DD）
 * 周日视为上周的周一。
 */
export function getWeekStart(dateStr: string): string {
  const [y, m, d] = dateStr.split("-").map(Number);
  const dt = new Date(y, m - 1, d, 12, 0, 0);
  const day = dt.getDay();
  const diff = (day === 0 ? -6 : 1) - day;
  dt.setDate(dt.getDate() + diff);
  const yy = dt.getFullYear();
  const mm = String(dt.getMonth() + 1).padStart(2, "0");
  const dd = String(dt.getDate()).padStart(2, "0");
  return `${yy}-${mm}-${dd}`;
}

/**
 * 获取当前小时（0-23，上海时区），用于问候语等
 */
export function currentHourShanghai(): number {
  const parts = new Intl.DateTimeFormat("zh-CN", {
    timeZone: "Asia/Shanghai",
    hour: "2-digit",
    hour12: false,
  }).formatToParts(new Date());
  const hour = parts.find((p) => p.type === "hour")?.value ?? "0";
  return parseInt(hour, 10);
}

/**
 * 获取当前时间在一天中的分钟数（0-1439，上海时区），用于与 HH:mm 字符串比较
 */
export function currentMinutesShanghai(): number {
  const parts = new Intl.DateTimeFormat("zh-CN", {
    timeZone: "Asia/Shanghai",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).formatToParts(new Date());
  const hour = parseInt(parts.find((p) => p.type === "hour")?.value ?? "0", 10);
  const minute = parseInt(parts.find((p) => p.type === "minute")?.value ?? "0", 10);
  return hour * 60 + minute;
}

/**
 * 将 HH:mm 字符串转换为分钟数（0-1439），无效格式返回 -1
 */
export function timeStringToMinutes(timeStr: string): number {
  const m = /^\s*(\d{1,2}):(\d{2})\s*$/.exec(timeStr);
  if (!m) return -1;
  const h = parseInt(m[1], 10);
  const min = parseInt(m[2], 10);
  if (h < 0 || h > 23 || min < 0 || min > 59) return -1;
  return h * 60 + min;
}
