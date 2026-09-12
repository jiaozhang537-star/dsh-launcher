/**
 * 版本选择页「收藏夹」的本地存储：实例 id 列表，持久于 localStorage。
 *
 * 收藏是启动器自身的轻量偏好（对标 PCL 的常用版本置顶心智），不进实例注册表；
 * 容忍脏数据——读取时过滤非字符串项，写入前去重。
 * store 参数化（同 hotkeys 范式）：单测注入内存实现。
 */

const KEY = "dsh-launcher:favorites";

/** 读取收藏的实例 id 列表（最新收藏在前）；无记录或解析失败返回空表。 */
export function loadFavorites(store: Storage = localStorage): string[] {
  try {
    const raw = store.getItem(KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return [...new Set(parsed.filter((v): v is string => typeof v === "string"))].reverse();
  } catch {
    return [];
  }
}

/** 覆写收藏列表（自动去重）。 */
export function saveFavorites(ids: readonly string[], store: Storage = localStorage): void {
  store.setItem(KEY, JSON.stringify([...new Set(ids)]));
}

/** 切换收藏状态，返回切换后是否为收藏。 */
export function toggleFavorite(id: string, store: Storage = localStorage): boolean {
  const ids = loadFavorites(store);
  if (ids.includes(id)) {
    saveFavorites(ids.filter((v) => v !== id), store);
    return false;
  }
  ids.push(id);
  saveFavorites(ids, store);
  return true;
}

/** 该实例是否被收藏。 */
export function isFavorite(id: string, store: Storage = localStorage): boolean {
  return loadFavorites(store).includes(id);
}

/** 实例删除后同步清掉收藏项。 */
export function dropFavorite(id: string, store: Storage = localStorage): void {
  saveFavorites(loadFavorites(store).filter((v) => v !== id), store);
}
