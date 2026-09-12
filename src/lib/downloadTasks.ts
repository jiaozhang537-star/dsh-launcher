/**
 * 下载任务的全局 store：跨页面切换存活（切走再切回任务卡不消失），
 * 完成/失败的任务保留到本次会话结束。页面只做订阅与渲染，状态都在这里。
 */

export type TaskStatus = "running" | "done" | "failed";
/** "source" = DSH 源码归档下载（FR-22，任务 key 前缀 `source-`，无实例）。 */
export type TaskPhase = "prepare" | "node" | "source" | "install" | "verify" | "done" | "failed";

export interface DownloadTask {
  instanceId: string;
  name: string;
  version: string;
  status: TaskStatus;
  phase: TaskPhase;
  /** 最近一行输出 / 阶段文案。 */
  detail: string;
  /** Node 自动下载字节进度（total=0 表示长度未知）。 */
  received: number;
  total: number;
  /** 计算出的下载速度（字节/秒）；null = 尚不可知。 */
  speed: number | null;
  /** 安装通道点名（如「正在从 https://registry.npmjs.org 安装 …」）。 */
  channel?: string;
  /** 开始时间戳（ms），供「已运行 xx」展示。 */
  startedAt: number;
}

const tasks = new Map<string, DownloadTask>();
/** 速度计算的上一次采样（≥300ms 才更新一次速度，避免抖动）。 */
const lastTick = new Map<string, { received: number; ts: number }>();
const listeners = new Set<() => void>();

function emit(): void {
  for (const listener of listeners) listener();
}

/** 订阅任务变化；返回取消订阅函数。 */
export function subscribeTasks(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** 当前任务快照（插入序 = 创建序）。 */
export function getTasks(): DownloadTask[] {
  return [...tasks.values()];
}

export function getTask(instanceId: string): DownloadTask | undefined {
  return tasks.get(instanceId);
}

/** 新建或整体覆盖任务。 */
export function upsertTask(task: DownloadTask): void {
  tasks.set(task.instanceId, { ...task });
  if (task.status !== "running") lastTick.delete(task.instanceId);
  emit();
}

/** 局部更新；任务不存在时忽略（进度事件晚于创建的竞态兜底）。 */
export function updateTask(instanceId: string, patch: Partial<Omit<DownloadTask, "instanceId">>): void {
  const current = tasks.get(instanceId);
  if (!current) return;
  tasks.set(instanceId, { ...current, ...patch });
  emit();
}

/**
 * 记录 Node 下载字节进度并计算速度（≥300ms 窗口的即时速率）。
 * 返回本次速度（字节/秒）；首个采样返回 null。
 */
export function recordProgress(instanceId: string, received: number, total: number): number | null {
  const now = Date.now();
  const prev = lastTick.get(instanceId);
  let speed: number | null = null;
  if (prev && now > prev.ts) {
    const dt = (now - prev.ts) / 1000;
    if (received >= prev.received && dt >= 0.3) {
      speed = (received - prev.received) / dt;
    }
  }
  lastTick.set(instanceId, { received, ts: now });
  const current = tasks.get(instanceId);
  if (current) {
    tasks.set(instanceId, { ...current, received, total, speed: speed ?? current.speed });
  }
  emit();
  return speed;
}

/** 清除全部任务（当前无入口，保留给后续「清除记录」使用）。 */
export function clearTasks(): void {
  tasks.clear();
  lastTick.clear();
  emit();
}

/**
 * 插件/整合包共用一条后端进度事件时的 spec 分流判据（审查 P1-04 的可测化）：
 * 状态行在发起安装时打上 `dataset.spec`，事件到达时只有 spec 相同的行被刷写；
 * 旧版 payload 无 spec（undefined/空串）向后兼容放行给当前接线行。
 */
export function progressMatchesSpec(
  lineSpec: string | undefined | null,
  eventSpec?: string,
): boolean {
  return !eventSpec || lineSpec === eventSpec;
}
