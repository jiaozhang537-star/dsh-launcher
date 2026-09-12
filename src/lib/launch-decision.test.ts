import { describe, expect, it } from "vitest";

import { webEntryUrl } from "./launch-decision";

describe("webEntryUrl", () => {
  it("优先用 dsh web 打印的带 token URL（新版鉴权栅栏只认它）", () => {
    expect(
      webEntryUrl({ webUrl: "http://127.0.0.1:3091/?token=deadbeef", port: 3091 }),
    ).toBe("http://127.0.0.1:3091/?token=deadbeef");
  });

  it("旧版本没打印 URL → 回落裸端口地址", () => {
    expect(webEntryUrl({ webUrl: null, port: 3091 })).toBe("http://127.0.0.1:3091");
  });

  it("URL 是空白串按没有处理 → 回落裸端口地址", () => {
    expect(webEntryUrl({ webUrl: "   ", port: 3082 })).toBe("http://127.0.0.1:3082");
  });

  it("既无 URL 又无端口 → null（宁可不打开，也不造一个必然打不开的地址）", () => {
    expect(webEntryUrl({ webUrl: null, port: null })).toBeNull();
  });
});
