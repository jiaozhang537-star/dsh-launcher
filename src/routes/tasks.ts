/**
 * 任务管理页（Phase 5 H3 · 需求方点名的「下载加载页」）。
 * 逐项对齐 PCL PageSpeed（PageSpeedLeft/Right 源码摘录见 phase-05 H 日志）：
 * 左栏统计块（总进度 20px 大字 / 下载速度 / 剩余任务）；右栏动态任务卡
 * （行 26px = 状态图标【进行中百分比/完成对勾/失败叉】+ 子任务文案；
 * 右上 X 取消/重试/打开目录）；全部落定 → Hint + 自动返回（TryReturnToHome）。
 */

import { el } from "../lib/dom";
import { icon } from "../lib/icons";
import { goBack, type PageDefinition } from "../router";
import { showHint } from "../lib/hint";
import {
  clearFinishedTasks,
  controlKeyOf,
  getTasks,
  isTaskActive,
  subscribeTasks,
  updateTask,
  type DownloadTask,
} from "../lib/downloadTasks";
import { openSourceDir } from "../services/sourceArchive";
import { cancelTask, pauseTask, resumeTask } from "../services/taskQueue";
import {
  fmtElapsed,
  isSourceTask,
  parseSourceTaskKey,
  startInstall,
  startSourceDownload,
} from "./downloads";

/** 字节速率口语化：12.3 MB/s。 */
function fmtSpeed(bytesPerSec: number): string {
  if (bytesPerSec >= 1024 * 1024) return `${(bytesPerSec / 1024 / 1024).toFixed(1)} MB/s`;
  if (bytesPerSec >= 1024) return `${(bytesPerSec / 1024).toFixed(0)} KB/s`;
  return `${bytesPerSec.toFixed(0)} B/s`;
}

/** 单任务进度百分比；无法量化（安装阶段无字节量）返回 null。 */
function taskPercent(task: DownloadTask): number | null {
  if (task.status === "done") return 100;
  if (!isTaskActive(task) || task.total <= 0) return null;
  return Math.min(100, (task.received / task.total) * 100);
}

/** PCL 状态图标：进行中=百分比文本；暂停=百分比（灰）；完成=对勾；失败/取消=叉。 */
function stateIcon(task: DownloadTask): HTMLElement {
  if (task.status === "done") return el("span", { class: "tasks__state tasks__state--done" }, [icon("check", 14)]);
  if (task.status === "failed") return el("span", { class: "tasks__state tasks__state--failed" }, [icon("x", 14)]);
  if (task.status === "cancelled") return el("span", { class: "tasks__state tasks__state--cancelled" }, [icon("x", 14)]);
  const pct = taskPercent(task);
  const cls = task.status === "paused" ? "tasks__state tasks__state--run tasks__state--paused" : "tasks__state tasks__state--run";
  return el("span", { class: cls }, [pct === null ? "…" : `${Math.floor(pct)}%`]);
}

/** 任务行：状态图标 + 文案 + 百分比 + 右上动作（暂停/继续/取消/重试/打开目录）。 */
function taskRow(task: DownloadTask, repaint: () => void): HTMLElement {
  const pct = taskPercent(task);
  const rowText =
    task.status === "paused"
      ? task.detail || "已暂停"
      : task.status === "running" && task.phase !== "node" && task.phase !== "source"
        ? `${fmtElapsed(task.startedAt)}${task.detail ? ` · ${task.detail}` : ""}`
        : task.detail || (task.status === "done" ? "完成" : "…");
  const row = el("div", { class: "tasks__row" }, [
    stateIcon(task),
    el("span", {
      class: `tasks__row-text${task.status === "failed" ? " tasks__row-text--failed" : ""}`,
      title: task.detail ?? "",
    }, [rowText]),
    pct !== null ? el("span", { class: "tasks__pct" }, [`${Math.floor(pct)}%`]) : document.createDocumentFragment(),
  ]);
  const actions = el("div", { class: "tasks__row-actions" });
  const key = controlKeyOf(task);
  // FR-09：字节流任务（canPause）给暂停/继续；所有活动任务都可取消。
  if (task.status === "running" && task.canPause) {
    const pause = el("button", { type: "button", class: "tasks__x", title: "暂停任务", "aria-label": "暂停任务" }, [icon("pause", 12)]);
    pause.addEventListener("click", () => {
      void pauseTask(key).catch(() => {});
      updateTask(task.instanceId, { status: "paused", detail: task.detail || "已暂停" });
      repaint();
    });
    actions.append(pause);
  }
  if (task.status === "paused") {
    const resume = el("button", { type: "button", class: "tasks__x", title: "继续任务", "aria-label": "继续任务" }, [icon("play", 12)]);
    resume.addEventListener("click", () => {
      void resumeTask(key).catch(() => {});
      updateTask(task.instanceId, { status: "running", detail: "" });
      repaint();
    });
    actions.append(resume);
  }
  if (isTaskActive(task)) {
    const cancel = el("button", { type: "button", class: "tasks__x", title: "取消任务", "aria-label": "取消任务" }, [icon("x", 12)]);
    cancel.addEventListener("click", () => {
      void cancelTask(key).catch(() => {});
      updateTask(task.instanceId, { detail: "正在取消…" });
      repaint();
    });
    actions.append(cancel);
  }
  if (task.status === "failed" || task.status === "cancelled") {
    const retry = el("button", { type: "button", class: "tasks__x", title: task.status === "cancelled" ? "重新下载（断点续传）" : "重试此任务", "aria-label": "重试此任务" }, [icon("refresh", 12)]);
    retry.addEventListener("click", () => {
      if (isSourceTask(task)) {
        const parsed = parseSourceTaskKey(task.instanceId);
        void startSourceDownload(parsed?.version ?? task.version, parsed?.instanceId ?? null);
      } else {
        void startInstall(task.instanceId, task.name, task.version);
      }
      repaint();
    });
    actions.append(retry);
  }
  if (task.status === "done" && isSourceTask(task)) {
    const open = el("button", { type: "button", class: "tasks__x", title: "打开源码目录", "aria-label": "打开源码目录" }, [icon("external-link", 12)]);
    open.addEventListener("click", () => {
      const parsed = parseSourceTaskKey(task.instanceId);
      void openSourceDir(parsed?.version ?? task.version, parsed?.instanceId ?? null).catch((err) => {
        updateTask(task.instanceId, { detail: `打开失败：${String(err).slice(0, 200)}` });
        repaint();
      });
    });
    actions.append(open);
  }
  row.append(actions);
  return row;
}

function paintPage(
  stats: { progress: HTMLElement; speed: HTMLElement; left: HTMLElement },
  cardsBox: HTMLElement,
  clearBtn: HTMLElement,
): void {
  const tasks = getTasks();
  const active = tasks.filter(isTaskActive);
  const running = tasks.filter((t) => t.status === "running");

  // 左栏统计（PCL PageSpeedLeft：总进度=各任务均值；速度=各任务速率和；
  // 剩余任务=活动数（运行+暂停——暂停的任务仍未结束）。
  const percents = active.map(taskPercent).filter((p): p is number => p !== null);
  const overall =
    active.length === 0
      ? null
      : percents.length > 0
        ? percents.reduce((a, b) => a + b, 0) / active.length
        : null;
  stats.progress.textContent =
    overall === null ? "--" : overall > 99.99 ? "100 %" : `${overall.toFixed(2)} %`;
  const speedSum = running.reduce((acc, t) => acc + (t.speed ?? 0), 0);
  stats.speed.textContent = running.some((t) => t.speed) ? fmtSpeed(speedSum) : "--";
  stats.left.textContent = `${active.length}`;
  // 清除记录：仅在有已结束（含取消）任务时出现（活动中的任务不受影响）。
  clearBtn.hidden = tasks.length === active.length;

  // 右栏任务卡（keyed：按 taskId 复用节点，事件高频不抖动）。
  const existing = new Map<string, HTMLElement>();
  for (const child of Array.from(cardsBox.children) as HTMLElement[]) {
    if (child.dataset.taskId) existing.set(child.dataset.taskId, child);
  }
  const seen = new Set<string>();
  for (const task of tasks) {
    seen.add(task.instanceId);
    const repaint = () => paintPage(stats, cardsBox, clearBtn);
    const prev = existing.get(task.instanceId);
    const card = el("section", { class: "card settings-card tasks-card" }, [
      el("div", { class: "tasks-card__head" }, [
        el("h2", { class: "card__title" }, [task.name]),
        el("span", { class: "tasks-card__meta" }, [fmtElapsed(task.startedAt)]),
      ]),
      taskRow(task, repaint),
    ]);
    card.dataset.taskId = task.instanceId;
    if (prev) {
      prev.replaceWith(card);
    } else {
      cardsBox.append(card);
    }
    existing.delete(task.instanceId);
  }
  for (const [, stale] of existing) stale.remove();
  if (tasks.length === 0) {
    // 空态只补一次：本页 500ms 轮询重绘，无条件 append 会每半秒堆一行灰字。
    if (cardsBox.children.length === 0) {
      cardsBox.append(el("p", { class: "settings-status" }, ["当前没有下载 / 安装任务——发起一个试试。"]));
    }
    return;
  }
}

/** 当前挂载的清理函数：unmount 钩子调用。 */
let stopActive: (() => void) | null = null;

/** 任务管理页（secondary：← 返回；不进导航）。 */
export const tasksPage: PageDefinition = {
  id: "tasks",
  navLabel: "任务管理",
  secondary: true,
  unmount: () => stopActive?.(),
  render: () => {
    stopActive?.();
    const stats = {
      progress: el("div", { class: "tasks__big" }, ["--"]),
      speed: el("div", { class: "tasks__stat-value" }, ["--"]),
      left: el("div", { class: "tasks__stat-value" }, ["0"]),
    };
    const cardsBox = el("div", { class: "tasks__cards" }, []);
    const clearBtn = el("button", { type: "button", class: "btn btn--ghost tasks__clear", hidden: "" }, ["清除记录"]);
    clearBtn.addEventListener("click", () => {
      clearFinishedTasks();
      paint();
    });
    const root = el("section", { class: "page" }, [
      el("div", { class: "tasks" }, [
        el("aside", { class: "tasks__left" }, [
          el("div", { class: "tasks__stat" }, [
            el("div", { class: "tasks__stat-label" }, ["总进度"]),
            stats.progress,
          ]),
          el("div", { class: "tasks__split" }),
          el("div", { class: "tasks__stat" }, [
            el("div", { class: "tasks__stat-label" }, ["下载速度"]),
            stats.speed,
          ]),
          el("div", { class: "tasks__split" }),
          el("div", { class: "tasks__stat" }, [
            el("div", { class: "tasks__stat-label" }, ["剩余任务"]),
            stats.left,
          ]),
          clearBtn,
        ]),
        el("section", { class: "tasks__right" }, [cardsBox]),
      ]),
    ]);

    let prevActive = -1;
    const paint = (): void => {
      paintPage(stats, cardsBox, clearBtn);
      const active = getTasks().filter(isTaskActive).length;
      if (prevActive > 0 && active === 0) {
        // 全部落定：Hint + 自动返回（PCL McInstallState + TryReturnToHome 同款）。
        showHint("全部任务已结束——正在返回");
        setTimeout(() => goBack(), 1200);
      }
      prevActive = active;
    };
    const unsub = subscribeTasks(() => paint());
    const timer = window.setInterval(paint, 500);
    paint();
    stopActive = () => {
      unsub();
      clearInterval(timer);
    };

    return root;
  },
};
