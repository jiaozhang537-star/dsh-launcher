/** 校验链报告的类型镜像（与 Rust precheck 对齐）。 */

export type Level = "error" | "warning";

export interface CheckItem {
  id: string;
  level: Level;
  message: string;
  action: string | null;
}

export interface PluginStats {
  installed: number;
  errors: number;
  warnings: number;
}

export interface Report {
  items: CheckItem[];
  blocked: boolean;
  plugins: PluginStats;
}

/** 从 instance_launch 的 reject 字符串里解析报告（Rust 侧把 Report JSON 化后当 Err 返回）。 */
export function parseBlockedReport(err: unknown): Report | null {
  if (typeof err !== "string") return null;
  const trimmed = err.trim();
  if (!trimmed.startsWith("{")) return null;
  try {
    const parsed = JSON.parse(trimmed) as Report;
    return Array.isArray(parsed.items) ? parsed : null;
  } catch {
    return null;
  }
}
