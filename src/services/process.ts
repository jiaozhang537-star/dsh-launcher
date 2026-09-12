/** Tauri invoke 的类型化薄封装：实例进程生命周期（FR-15/FR-08）。 */

import type { Report } from "./precheck";

export interface RunStatus {
  instanceId: string;
  port: number;
  pid: number;
}

export interface LaunchOutcome {
  instanceId: string;
  port: number;
  pid: number;
}

/** 启动答复：进程信息 + 校验链报告（加载动画据此渲染体检通过/告警清单）。 */
export interface LaunchResult extends LaunchOutcome {
  report: Report;
  /** 本次启动的实例配置对账（插件/Skill/外部提供商相对上次启动的变更）。 */
  reconcile: Reconcile;
}

/** 启动时对账结果（后端 `snapshot::Reconcile`）。 */
export interface Reconcile {
  /** false = 首次启动（无基线）：只报构成，不报新增/移除。 */
  baseline: boolean;
  pluginsTotal: number;
  skillsTotal: number;
  externalRoutesTotal: number;
  pluginsAdded: string[];
  pluginsRemoved: string[];
  skillsAdded: string[];
  skillsRemoved: string[];
  externalRoutesAdded: string[];
  externalRoutesRemoved: string[];
  /** 对账自身的问题（采集/落盘失败）——如实呈现，不阻断启动。 */
  note: string | null;
}

/** 启动进度（加载动画的百分比与阶段文案由后端真实进度驱动）。 */
export interface LaunchProgress {
  instanceId: string;
  /** 0~100；等待网页就绪期间在 70~96 缓升，就绪时 100。 */
  percent: number;
  stage: string;
}

export interface ConsoleLine {
  instanceId: string;
  stream: "stdout" | "stderr";
  line: string;
}

export interface ExitNotice {
  instanceId: string;
  code: number;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(cmd, args);
}

/** 运行启动前校验链。 */
export function runPrecheck(instanceId: string): Promise<Report> {
  return invoke<Report>("precheck_run", { id: instanceId });
}

/**
 * 启动实例；校验链有错误时默认拒绝——reject 的 err 是 JSON 化的 Report，
 * 前端 parse 后可展示报告并以 force=true 重试。
 *
 * 每次调用都是一次独立的完整启动：实例在跑时由**后端**先停旧进程（含上一会话
 * 遗留的孤儿）并等钉定端口释放，再重新拉起——前端不做"是否只开网页"的决策。
 */
export function launchInstance(instanceId: string, force = false): Promise<LaunchResult> {
  return invoke<LaunchResult>("instance_launch", { id: instanceId, force });
}

/**
 * 取消本次启动：后端登记取消令牌（启动链下一个检查点就地收尾），并把已经拉起的
 * 进程停掉（取消可能落在「进程正在拉起」的窗口里）。幂等：没在跑也返回成功。
 *
 * 前端不靠错误文案判断取消——自己在点取消时置 `cancelRequested`，据此决定
 * 是「启动失败」还是「用户取消」。
 */
export function cancelLaunch(instanceId: string): Promise<void> {
  return invoke<void>("instance_launch_cancel", { id: instanceId });
}

/** 实例进程状态（启动按钮的决策输入）。 */
export interface ProcessState {
  instanceId: string;
  /** 进程仍在运行：本会话拉起的，或本启动器此前拉起且仍存活的。 */
  running: boolean;
  /** true = 上一会话遗留的孤儿进程（已按运行记录采用）。 */
  adopted: boolean;
  pid: number | null;
  port: number | null;
  /** 插件装配自该进程拉起以来变过——不重启进程不会生效。 */
  restartNeeded: boolean;
  /**
   * `dsh web` 打印的网页 URL（带进程 token）。**打开网页必须用它**：新版 DSH
   * 有鉴权栅栏，裸 `http://127.0.0.1:<port>/` 一律 401。null = 旧版本没打印，
   * 此时裸端口 URL 才是正确入口。
   */
  webUrl: string | null;
}

/** 查询实例进程状态（含孤儿识别与重启判定）。 */
export function getProcessState(instanceId: string): Promise<ProcessState> {
  return invoke<ProcessState>("instance_process_state", { id: instanceId });
}

/** 停止实例进程树（幂等）。 */
export function stopInstance(instanceId: string): Promise<void> {
  return invoke<void>("instance_stop", { id: instanceId });
}

/** 当前运行中的实例。 */
export function listRunning(): Promise<RunStatus[]> {
  return invoke<RunStatus[]>("instance_running_list");
}

/** 订阅控制台输出行。 */
export async function onConsoleLine(handler: (line: ConsoleLine) => void): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<ConsoleLine>("instance-console", (event) => handler(event.payload));
}

/** 订阅诊断卡片事件。 */
export async function onDiagnostic(
  handler: (payload: { instanceId: string; diagnostic: DiagnosticCard }) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<{ instanceId: string; diagnostic: DiagnosticCard }>(
    "instance-diagnostic",
    (event) => handler(event.payload),
  );
}

/** 订阅实例退出事件。 */
export async function onExit(handler: (notice: ExitNotice) => void): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<ExitNotice>("instance-exit", (event) => handler(event.payload));
}

export interface WebReadyNotice {
  instanceId: string;
  port: number;
  /**
   * `dsh web` 就绪时打印的带 token URL（后端从子进程 stdout 抓取）。
   * null/缺省 = 该版本不打印（旧版本），前端回落 `http://127.0.0.1:<port>/`。
   */
  url?: string | null;
}

/** 订阅「实例网页就绪」事件（R6）：后端的就绪判定为准，前端据此才开浏览器。 */
export async function onWebReady(handler: (notice: WebReadyNotice) => void): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<WebReadyNotice>("instance-web-ready", (event) => handler(event.payload));
}

/** 订阅「网页就绪超时」事件（180s 未连通——实例可能仍在初始化）。 */
export async function onWebTimeout(
  handler: (notice: WebReadyNotice) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<WebReadyNotice>("instance-web-timeout", (event) => handler(event.payload));
}

/** 订阅启动进度事件（校验 → 拉起进程 → 等待网页就绪 → 就绪）。 */
export async function onLaunchProgress(
  handler: (progress: LaunchProgress) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<LaunchProgress>("launch-progress", (event) => handler(event.payload));
}

/** 诊断卡片形态（与 Rust Diagnostic 对齐）。 */
export interface DiagnosticCard {
  level: string;
  title: string;
  detail: string;
}
