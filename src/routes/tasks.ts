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
  getTasks,
  subscribeTasks,
  updateTask,
  type DownloadTask,
} from "../lib/downloadTasks";
import { cancelSourceDownload, openSourceDir } from "../services/sourceArchive";
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
  if (task.status !== "running" || task.total <= 0) return null;
  return Math.min(100, (task.received / task.total) * 100);
}

/** PCL 状态图标：进行中=百分比文本；完成=对勾；失败=叉。 */
function stateIcon(task: DownloadTask): HTMLElement {
  if (task.status === "done") return el("span", { class: "tasks__state tasks__state--done" }, [icon("check", 14)]);
  if (task.status === "failed") return el("span", { class: "tasks__state tasks__state--failed" }, [icon("x", 14)]);
  const pct = taskPercent(task);
  return el("span", { class: "tasks__state tasks__state--run" }, [pct === null ? "…" : `${Math.floor(pct)}%`]);
}

/** 任务行：状态图标 + 文案 + 百分比 + 右上动作（取消/重试/打开目录）。 */
function taskRow(task: DownloadTask, repaint: () => void): HTMLElement {
  const pct = taskPercent(task);
  const row = el("div", { class: "tasks__row" }, [
    stateIcon(task),
    el("span", {
      class: `tasks__row-text${task.status === "failed" ? " tasks__row-text--failed" : ""}`,
      title: task.detail ?? "",
    }, [
      task.status === "running" && task.phase !== "node" && task.phase !== "source"
        ? `${fmtElapsed(task.startedAt)}${task.detail ? ` · ${task.detail}` : ""}`
        : task.detail || (task.status === "done" ? "完成" : "…"),
    ]),
    pct !== null ? el("span", { class: "tasks__pct" }, [`${Math.floor(pct)}%`]) : document.createDocumentFragment(),
  ]);
  const actions = el("div", { class: "tasks__row-actions" });
  if (task.status === "running" && isSourceTask(task)) {
    const cancel = el("button", { type: "button", class: "tasks__x", title: "取消下载", "aria-label": "取消下载" }, [icon("x", 12)]);
    cancel.addEventListener("click", () => {
      void cancelSourceDownload();
      updateTask(task.instanceId, { detail: "正在取消…" });
      repaint();
    });
    actions.append(cancel);
  }
  if (task.status === "failed") {
    const retry = el("button", { type: "button", class: "tasks__x", title: "重试此任务", "aria-label": "重试此任务" }, [icon("refresh", 12)]);
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

function paintPage(stats: { progress: HTMLElement; speed: HTMLElement; left: HTMLElement }, cardsBox: HTMLElement): void {
  const tasks = getTasks();
  const running = tasks.filter((t) => t.status === "running");

  // 左栏统计（PCL PageSpeedLeft：总进度=各任务均值；速度=各任务速率和）。
  const percents = running.map(taskPercent).filter((p): p is number => p !== null);
  const overall =
    running.length === 0
      ? null
      : percents.length > 0
        ? percents.reduce((a, b) => a + b, 0) / running.length
        : null;
  stats.progress.textContent =
    overall === null ? "--" : overall > 99.99 ? "100 %" : `${overall.toFixed(2)} %`;
  const speedSum = running.reduce((acc, t) => acc + (t.speed ?? 0), 0);
  stats.speed.textContent = running.some((t) => t.speed) ? fmtSpeed(speedSum) : "--";
  stats.left.textContent = `${running.length}`;

  // 右栏任务卡（keyed：按 taskId 复用节点，事件高频不抖动）。
  const existing = new Map<string, HTMLElement>();
  for (const child of Array.from(cardsBox.children) as HTMLElement[]) {
    if (child.dataset.taskId) existing.set(child.dataset.taskId, child);
  }
  const seen = new Set<string>();
  for (const task of tasks) {
    seen.add(task.instanceId);
    const repaint = () => paintPage(stats, cardsBox);
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
        ]),
        el("section", { class: "tasks__right" }, [cardsBox]),
      ]),
    ]);

    let prevRunning = -1;
    const paint = (): void => {
      paintPage(stats, cardsBox);
      const running = getTasks().filter((t) => t.status === "running").length;
      if (prevRunning > 0 && running === 0) {
        // 全部落定：Hint + 自动返回（PCL McInstallState + TryReturnToHome 同款）。
        showHint("全部任务已结束——正在返回");
        setTimeout(() => goBack(), 1200);
      }
      prevRunning = running;
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
