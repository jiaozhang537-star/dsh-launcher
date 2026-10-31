import { describe, expect, it } from "vitest";
import { toFriendlyError } from "./tauri";

/**
 * 统一错误出口的映射对照（对抗审查 R1 P1-05）：已知英文错误模式必须收敛成
 * 中文友好句，不得漏英文原文进 UI；Rust 侧中文文案原样透传。
 */
describe("toFriendlyError", () => {
  it("网络类错误统一收敛为网络失败提示", () => {
    expect(toFriendlyError("error sending request: connection refused")).toContain("网络连接失败");
    expect(toFriendlyError("operation timed out")).toContain("网络连接失败");
    expect(toFriendlyError("request timeout")).toContain("网络连接失败");
    expect(toFriendlyError("connection reset by peer")).toContain("网络连接失败");
    expect(toFriendlyError("broken pipe")).toContain("网络连接失败");
  });

  it("GitHub 限流映射为稍后重试", () => {
    expect(toFriendlyError("HTTP 403 rate limit exceeded")).toContain("GitHub 请求受限");
    expect(toFriendlyError("HTTP 429 Too Many Requests")).toContain("GitHub 请求受限");
  });

  it("文件系统错误映射为中文指引（os error 码不再裸露）", () => {
    expect(toFriendlyError("No such file or directory (os error 2)")).toContain("找不到所需的文件");
    expect(toFriendlyError("Access is denied (os error 5)")).toContain("没有访问权限");
    expect(toFriendlyError("permission denied")).toContain("没有访问权限");
  });

  it("序列化与代理错误各有出口", () => {
    expect(toFriendlyError("invalid type: string, expected u16")).toContain("解析失败");
    expect(toFriendlyError("deserializing from JSON failed")).toContain("解析失败");
    expect(toFriendlyError("proxy connect failed")).toContain("代理连接失败");
  });

  it("取消语义中英文都收敛为「已取消」", () => {
    expect(toFriendlyError("已取消")).toBe("已取消");
    expect(toFriendlyError("download cancelled")).toBe("已取消");
    expect(toFriendlyError("operation aborted")).toBe("已取消");
  });

  it("既有映射不回归", () => {
    expect(toFriendlyError("Command not found")).toContain("功能未就绪");
    expect(toFriendlyError("invalid args `id`: invalid string")).toContain("调用参数错误");
    expect(toFriendlyError("npm ERR! code ELIFECYCLE")).toContain("安装失败");
    expect(toFriendlyError("服务返回 HTTP 500 错误")).toContain("HTTP 500");
  });

  it("Rust 中文文案与未知文本原样透传", () => {
    expect(toFriendlyError("实例不存在：i-abc")).toBe("实例不存在：i-abc");
    expect(toFriendlyError("something unusual")).toBe("something unusual");
    expect(toFriendlyError("")).toBe("操作未完成");
  });
});
