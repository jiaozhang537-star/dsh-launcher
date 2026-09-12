/** 插件市场「类型」筛选的值域契约。
 *
 * 后端 `market.rs::validate_query` 只认 `all` 或七个中文分类（CATEGORIES），
 * 前端 option 的 value 必须落在同一域内。历史缺陷：option 用中文标签当 value
 * （"全部"），用户一旦动过类型下拉，整页就报「非法类型筛选」。 */
import { describe, expect, it } from "vitest";
import { KIND_OPTIONS } from "./download-plugins";

describe("插件市场类型筛选", () => {
  it("value 域 = all + 七个中文分类（与后端白名单一致）", () => {
    expect(KIND_OPTIONS.map((option) => option.value)).toEqual([
      "all",
      "界面",
      "搜索",
      "机器人",
      "记忆",
      "视觉",
      "开发",
      "效率",
      "其他",
    ]);
  });

  it("没有任何选项把中文标签当 value 传给后端", () => {
    expect(KIND_OPTIONS.some((option) => option.value === "全部")).toBe(false);
  });

  it("「全部」的值是 all 且是首项（下拉默认态）", () => {
    expect(KIND_OPTIONS[0]).toEqual({ value: "all", label: "全部" });
  });
});
