import { beforeEach, describe, expect, it } from "vitest";
import { dropFavorite, isFavorite, loadFavorites, toggleFavorite } from "./instanceFavorites";

/** 内存版 Storage（node 测试环境无 localStorage，同 hotkeys 注入范式）。 */
function memStore(): Storage {
  const map = new Map<string, string>();
  return {
    get length() {
      return map.size;
    },
    clear: () => map.clear(),
    getItem: (k) => map.get(k) ?? null,
    key: (i) => [...map.keys()][i] ?? null,
    removeItem: (k) => map.delete(k),
    setItem: (k, v) => map.set(k, String(v)),
  };
}

let store: Storage;

beforeEach(() => {
  store = memStore();
});

describe("loadFavorites", () => {
  it("无记录返回空表", () => {
    expect(loadFavorites(store)).toEqual([]);
  });

  it("损坏数据返回空表而不抛错", () => {
    store.setItem("dsh-launcher:favorites", "{not json");
    expect(loadFavorites(store)).toEqual([]);
    store.setItem("dsh-launcher:favorites", '{"a":1}');
    expect(loadFavorites(store)).toEqual([]);
  });

  it("过滤非字符串项并去重（最新收藏在前）", () => {
    store.setItem("dsh-launcher:favorites", JSON.stringify(["a", 1, "a", null, "b"]));
    expect(loadFavorites(store)).toEqual(["b", "a"]);
  });
});

describe("toggleFavorite", () => {
  it("收藏→取消→再收藏，返回状态并持久化", () => {
    expect(toggleFavorite("i-1", store)).toBe(true);
    expect(isFavorite("i-1", store)).toBe(true);
    expect(toggleFavorite("i-1", store)).toBe(false);
    expect(isFavorite("i-1", store)).toBe(false);
    toggleFavorite("i-1", store);
    expect(loadFavorites(store)).toEqual(["i-1"]);
  });
});

describe("dropFavorite", () => {
  it("删除实例后同步清理收藏项", () => {
    toggleFavorite("i-1", store);
    toggleFavorite("i-2", store);
    dropFavorite("i-1", store);
    expect(loadFavorites(store)).toEqual(["i-2"]);
  });
});
