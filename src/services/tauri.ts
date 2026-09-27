/**
 * Tauri invoke 统一出口（Phase 5 H2 · 需求方批评「给用户看英文报错」）。
 * 所有 services 文件经此调用后端：错误一律规范化为**中文友好句**再 reject，
 * UI 的 `String(err)` 零改动即得中文；原文 console.error 留档（不进 UI）。
 */

let warnedNotTauri = false;

/** 是否运行在启动器 WebView 内（浏览器预览页无 IPC）。 */
export function isTauri(): boolean {
  return "__TAURI_INTERNALS__" in (window as unknown as Record<string, unknown>);
}

/** 错误原文 → 中文友好句。命中模式才替换；Rust 侧本就中文的文案原样透传。 */
export function toFriendlyError(raw: unknown): string {
  const text = typeof raw === "string" ? raw : raw instanceof Error ? raw.message : String(raw);
  if (text.includes("已取消")) return "已取消";
  const t = text.toLowerCase();
  if (t.includes("not found") && t.includes("command")) {
    return "功能未就绪——请重启启动器后重试";
  }
  if (t.includes("invalid args")) return "调用参数错误——请重启启动器后重试";
  if (
    t.includes("cannot read properties of undefined") ||
    t.includes("is not a function") ||
    t.includes("__tauri_internals__")
  ) {
    return "页面脚本异常——请重启启动器后重试";
  }
  if (
    t.includes("error sending request") ||
    t.includes("dns error") ||
    t.includes("connection refused") ||
    t.includes("timed out") ||
    t.includes("networkchanged") ||
    t.includes("network error")
  ) {
    return "网络连接失败——请检查网络，或在「设置 · 配置」里调整镜像源 / Token";
  }
  if (t.includes("403") || t.includes("429") || t.includes("rate limit")) {
    return "GitHub 请求受限——可在「设置 · 配置」里配置 Token 提额";
  }
  const http = t.match(/http (\d{3})/);
  if (http) {
    return `服务返回错误（HTTP ${http[1]}）——稍后重试，或在设置里检查镜像源`;
  }
  if (t.includes("pnpm") || t.includes("npm err")) {
    return "安装失败（依赖安装报错，详见控制台日志）";
  }
  if (text.trim() === "") return "操作未完成";
  return text;
}

/** 规范化的 invoke：非 Tauri 环境/失败时 reject 中文友好句，原文进控制台。 */
export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    if (!warnedNotTauri) {
      warnedNotTauri = true;
      console.warn("[tauri] 当前不在启动器窗口内运行，invoke 已降级为拒绝");
    }
    throw "当前不在启动器窗口内运行";
  }
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  try {
    return await tauriInvoke<T>(cmd, args);
  } catch (err) {
    const friendly = toFriendlyError(err);
    console.error(`[${cmd}]`, err);
    throw friendly;
  }
}
