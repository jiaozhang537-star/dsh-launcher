import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * 编码守卫：项目内的文本文件一律不得以 UTF-8 BOM 开头。
 *
 * 2026-09-10 真机事故：手工改 `instances.json` 的端口时，用 PowerShell 5.1 的
 * `Set-Content -Encoding UTF8` 写进了 BOM（首三字节 EF BB BF）。`serde_json`
 * 不接受 BOM，于是登记表整体读不出来——启动页红字「页面数据加载失败」、建实例
 * /装版本/装插件全部失效；同一批还把 5 个已提交文件（含 Rust 源码与文档）写脏。
 * Rust 编译器容忍 BOM，所以源码侧不会报错，只有靠这道守卫才拦得住。
 *
 * 只判 BOM，不判行尾：`src/styles/main.css` 既有全 CRLF 风格，不是缺陷。
 */

/** 参与检查的文本扩展名。 */
const TEXT_EXTENSIONS = [".rs", ".ts", ".js", ".json", ".md", ".css", ".html", ".toml", ".yml", ".yaml"];

/** 不参与检查的目录（构建产物、依赖、临时工作区）。 */
const SKIP_DIRS = new Set(["node_modules", "target", "dist", ".git", ".recon", "coverage"]);

function collectTextFiles(root: string, dir: string, out: string[]): void {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (SKIP_DIRS.has(entry.name)) continue;
      collectTextFiles(root, join(dir, entry.name), out);
      continue;
    }
    if (!entry.isFile()) continue;
    if (!TEXT_EXTENSIONS.some((ext) => entry.name.endsWith(ext))) continue;
    out.push(relative(root, join(dir, entry.name)));
  }
}

/** 项目根：vitest 以项目根为工作目录运行。 */
const projectRoot = resolve(process.cwd());

describe("编码守卫", () => {
  it("项目内文本文件不得以 UTF-8 BOM 开头", () => {
    const files: string[] = [];
    for (const dir of ["src", "src-tauri/src", "docs"]) {
      const abs = join(projectRoot, dir);
      try {
        if (statSync(abs).isDirectory()) collectTextFiles(projectRoot, abs, files);
      } catch {
        // 目录不存在则跳过（例如裁剪过的检出），不因此判失败。
      }
    }
    // 目录清单本身必须非空，否则守卫会因「扫不到文件」而假绿。
    expect(files.length).toBeGreaterThan(20);

    const offenders = files.filter((rel) => readFileSync(join(projectRoot, rel)).subarray(0, 3).equals(Buffer.from([0xef, 0xbb, 0xbf])));

    expect(offenders, `以下文件带 UTF-8 BOM，请去掉（BOM 会让 JSON 解析报 expected value）：\n${offenders.join("\n")}`).toEqual([]);
  });
});
