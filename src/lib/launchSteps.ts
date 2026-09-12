/**
 * 加载页字幕序列的纯逻辑：后端等级 → 字幕行等级、进行中阶段 → 完成态措辞。
 *
 * 阶段文案的原文由 Rust 侧给出（`lib.rs` 的 `emit_launch_progress` 五处 +
 * `process.rs` 等待网页一段）——改那边的中文请同步这里的 `STAGE_DONE`；
 * 表里没有的阶段一律原样显示，不替后端编词。
 */

/**
 * 字幕行的类型（决定左侧图标，不决定文案）：
 * - `ok`：**启动器自己下的结论**（阶段完成、对账结果、网页已就绪）；
 * - `info`：后端的 Warning 级条目原样呈现。它的语义是「展示后可放行」，其中既有
 *   「托管 Node 就绪」这类说明性结论、也有插件版本约束这类真提示——级别由后端定，
 *   前端既不升格成通过、也不降格成问题；
 * - `note`：后端的 Error 级条目，以及拦截/失败/异常退出。
 */
export type StepLevel = "ok" | "info" | "note";

/** 校验报告条目等级 → 字幕行等级（后端只发 error / warning 两级）。 */
export function stepLevelOf(reportLevel: "error" | "warning"): StepLevel {
  return reportLevel === "error" ? "note" : "info";
}

/**
 * 进行中的阶段文案 → 收尾后的完成态文案。
 *
 * 进行时的措辞（「正在校验运行环境…」）留在一行**已完成**的清单里读起来是错的，
 * 而这一行是用户唯一能回看的阶段记录，所以收尾时换成「做完了」的说法。
 */
const STAGE_DONE: Record<string, string> = {
  "正在校验运行环境…": "运行环境校验通过",
  "正在对账实例配置…": "实例配置已对账",
  "正在同步模型配置…": "模型配置已同步",
  "实例正在运行，正在重启实例…": "旧进程已停止，按原端口重启",
  "正在拉起实例进程…": "实例进程已拉起",
  "等待 DSH 装载插件层并开放网页…": "DSH 已装载插件层并开放网页",
};

/** 阶段行收尾时的文案；不认识的阶段原样返回（不编造完成态）。 */
export function settledStageText(stage: string): string {
  return STAGE_DONE[stage] ?? stage;
}
