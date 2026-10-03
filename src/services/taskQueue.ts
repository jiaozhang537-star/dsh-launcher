/** Tauri invoke 的类型化薄封装：下载任务控制（FR-09 W1，task_queue.rs）。 */
import { invoke } from "./tauri";

/** 暂停任务（key = 任务 store 的 controlKeyOf 结果）。字节流任务在下一个读块生效。 */
export function pauseTask(key: string): Promise<void> {
  return invoke<void>("task_pause", { key });
}

/** 继续任务：连接仍存活则原连接继续，否则后端按 Range 断点自动重连续传。 */
export function resumeTask(key: string): Promise<void> {
  return invoke<void>("task_resume", { key });
}

/** 取消任务：字节流保留 `.part` 断点（重试自动续传），子进程链杀进程收尾。 */
export function cancelTask(key: string): Promise<void> {
  return invoke<void>("task_cancel", { key });
}
