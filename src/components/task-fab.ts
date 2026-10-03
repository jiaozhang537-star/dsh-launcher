/**
 * 右下任务浮窗（Phase 5 H3 · PCL MyExtraButton 同构，需求方说的
 * 「像 Windows 重启的按钮」）：Ø40 主色圆钮 + 白 18% 进度填充（自下而上）+
 * 波纹动画；有进行中任务时浮现，点击进任务管理页，任务页自身打开时隐藏。
 * 全局单例，main.ts 挂载到 app-shell（Hint 宿主之下、弹窗层之下）。
 */

import { el } from "../lib/dom";
import { icon } from "../lib/icons";
import { navigate, pageById } from "../router";
import { getTasks, isTaskActive, subscribeTasks, type DownloadTask } from "../lib/downloadTasks";

/** 汇总进度：有字节量的活动任务（运行+暂停）取均值；无可量化任务时 0。 */
function overallProgress(active: DownloadTask[]): number {
  const withTotal = active.filter((t) => t.total > 0);
  if (withTotal.length === 0) return 0;
  const avg =
    withTotal.reduce((acc, t) => acc + Math.min(100, (t.received / t.total) * 100), 0) /
    withTotal.length;
  return Math.max(0, Math.min(100, avg));
}

/** 全局挂载（main.ts 调用一次）。 */
export function mountTaskFab(host: HTMLElement): void {
  const fill = el("span", { class: "task-fab__fill" });
  const ripple = el("span", { class: "task-fab__ripple" });
  const fab = el("button", {
    type: "button",
    class: "task-fab",
    title: "任务管理",
    "aria-label": "任务管理",
  }, [icon("download", 18), fill, ripple]);
  fab.addEventListener("click", () => navigate("tasks"));
  host.append(fab);

  let prevActive = -1;
  const paint = (): void => {
    const active = getTasks().filter(isTaskActive);
    const onPage = pageById("tasks") !== undefined && location.hash === "#/tasks";
    const show = active.length > 0 && !onPage;
    fab.classList.toggle("task-fab--show", show);
    fill.style.height = `${overallProgress(active)}%`;
    // 状态跃迁（新任务出现/全部落定）触发波纹（PCL Ribble 同款）。
    if (prevActive !== active.length && show) {
      ripple.classList.remove("task-fab__ripple--go");
      void ripple.offsetWidth;
      ripple.classList.add("task-fab__ripple--go");
    }
    prevActive = active.length;
  };
  subscribeTasks(() => paint());
  // hash 变化也要重算显隐（进任务页时隐藏）——router 无公开 hash 订阅，
  // 借 500ms 低频轮询补齐（开销可忽略）。
  window.setInterval(paint, 500);
  paint();
}
