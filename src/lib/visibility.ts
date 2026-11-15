import { getCurrentWindow, type Window } from "@tauri-apps/api/window";
import { getLauncherVisibility } from "../services/launcherConfig";
import { listRunning, onExit } from "../services/process";

/**
 * 启动器可见性（Phase 8.5，FR-28；PCL LaunchArgumentVisible 同语义，「游戏」
 * 映射「实例」）：网页就绪后对启动器自身窗口施加 关闭/隐藏/最小化/保持不变；
 * 隐藏态下最后一个实例退出后自动关闭（档 2）或重新打开（档 3）。
 * 仅作用于启动器窗口——网页仍经系统浏览器打开（需求方定稿不变）。
 */

/** 可见性档位 → 网页就绪时的窗口动作（PCL 值系 0/2/3/4/5，1 弃用）。 */
export function visibilityOp(
  value: number | null | undefined,
): "exit" | "hide" | "minimize" | "keep" {
  switch (value) {
    case 0:
      return "exit";
    case 2:
    case 3:
      return "hide";
    case 4:
      return "minimize";
    default:
      return "keep";
  }
}

/**
 * 实例退出时的裁决（纯函数便于单测）：仅当**最后一个**实例退出才有动作——
 * 档 2 关闭启动器、档 3 重新打开；仍有实例在跑或档位无关时不动。
 * PCL 的触发是「该局游戏退出」，多实例并存下泛化为「全部实例退出」。
 */
export function exitHookDecision(
  value: number | null | undefined,
  runningCount: number,
): "exit" | "show" | "none" {
  if (runningCount > 0) return "none";
  const v = value ?? 5;
  if (v === 2) return "exit";
  if (v === 3) return "show";
  return "none";
}

/** 仅在 Tauri WebView 内生效；纯浏览器预览时窗口控制为无操作（topbar 同款）。 */
function isRunningInTauri(): boolean {
  return "__TAURI_INTERNALS__" in (window as unknown as Record<string, unknown>);
}

async function withWindow(action: (win: Window) => Promise<void>): Promise<void> {
  if (!isRunningInTauri()) return;
  try {
    await action(getCurrentWindow());
  } catch (error) {
    console.error("窗口操作失败", error);
  }
}

/** 读当前档位；读失败 = 未配置，按保持不变处理（不出声，默认项不该吵）。 */
async function currentVisibility(): Promise<number | null> {
  try {
    return await getLauncherVisibility();
  } catch {
    return null;
  }
}

/** 网页就绪（开浏览器之后）调用：按档位施加启动器窗口动作。 */
export async function applyVisibilityOnLaunch(): Promise<void> {
  const op = visibilityOp(await currentVisibility());
  if (op === "exit") {
    // 立即关闭（PCL 档 0）：DSH 进程不随启动器退出而终止（孤儿持久设计），安全。
    await withWindow((win) => win.close());
  } else if (op === "hide") {
    await withWindow((win) => win.hide());
  } else if (op === "minimize") {
    await withWindow((win) => win.minimize());
  }
}

/** 应用启动期挂一次：最后一个实例退出时按档位关闭/重新打开启动器。 */
export function initVisibilityLifecycle(): void {
  void onExit(async () => {
    const [value, running] = await Promise.all([
      currentVisibility(),
      // 读不到运行态 = 未知，按仍有实例处理，不误关。
      listRunning()
        .then((list) => list.length)
        .catch(() => 1),
    ]);
    const decision = exitHookDecision(value, running);
    if (decision === "exit") {
      await withWindow((win) => win.close());
    } else if (decision === "show") {
      await withWindow(async (win) => {
        await win.show();
        await win.unminimize();
        await win.setFocus();
      });
    }
  });
}
