/**
 * 实例列表的纯函数工具（无 DOM、无 invoke）。
 *
 * 背景：launch.ts 与 instance-select.ts 曾各自内联同一排序比较器，且在字段
 * 缺失时对 undefined 调 localeCompare 直接抛 TypeError，把两个页面的初始化
 * 整体炸掉。此处集中实现并做防御式回落，两个路由共用，vitest 覆盖。
 */

import type { Instance } from "../services/instances";
import { loadFavorites } from "./instanceFavorites";

/** 「最近使用置顶」排序：lastLaunchedAt 优先，其次 createdAt，均缺回落空串。 */
export function sortInstancesByRecency(list: Instance[]): Instance[] {
  const keyOf = (inst: Instance): string => inst.lastLaunchedAt ?? inst.createdAt ?? "";
  return [...list].sort((a, b) => keyOf(b).localeCompare(keyOf(a)));
}

/**
 * FR-10 置顶排序（2026-09-30 裁决：收藏 = 置顶，两概念合一，不新增 pinned 字段）：
 * 收藏的实例按收藏先后置顶，其余按最近使用。favorites 可注入（vitest 用），
 * 缺省读 localStorage。启动页默认选中与列表序共用此口径。
 */
export function sortInstancesForLaunch(
  list: Instance[],
  favorites: readonly string[] = loadFavorites(),
): Instance[] {
  const rankOf = (id: string): number => {
    const idx = favorites.indexOf(id);
    return idx === -1 ? Number.MAX_SAFE_INTEGER : idx;
  };
  const keyOf = (inst: Instance): string => inst.lastLaunchedAt ?? inst.createdAt ?? "";
  return [...list].sort(
    (a, b) => rankOf(a.id) - rankOf(b.id) || keyOf(b).localeCompare(keyOf(a)),
  );
}

/**
 * FR-19 命名预演：base 被占用时自动加 `-2/-3/…` 后缀直到唯一。
 * 与后端 unique_name 同规则；后端仍是权威兜底，这里只消除确认视图的预期落差。
 */
export function uniqueInstanceName(base: string, takenNames: readonly string[]): string {
  const taken = new Set(takenNames);
  if (!taken.has(base)) return base;
  for (let n = 2; ; n += 1) {
    const candidate = `${base}-${n}`;
    if (!taken.has(candidate)) return candidate;
  }
}
