/** DSH 网页端原版快捷键目录（设置·热键页只读展示，2026-09-25 需求方裁决）。
 *
 * 热键只有一个含义：DSH 网页端各操作的快捷键。DSH 前端键位全部硬编码、无任何
 * 键位配置机制（三代实测一致：0.1.1-rc.2 / 0.1.5-rc.1 / 0.1.5-rc.3，全源码
 * hotkey|keybinding 0 命中），因此本目录是随启动器内置的静态数据：启动器不提供
 * 修改，也不展示无键操作。逐条源码证据登记在
 * docs/plans/phase-04.5-gap-sweep-and-strategy.md 附表；DSH 升级后须复检并更新
 * BASELINE_DSH_VERSION。 */

export type DshHotkeyGroupId = "composer" | "trigger-menu" | "panels" | "trees";

export interface DshHotkeyGroup {
  id: DshHotkeyGroupId;
  label: string;
}

export interface DshHotkeyEntry {
  id: string;
  label: string;
  /** 生效按键（数组内为并列可用的键位/形态，全部展示为键帽）。 */
  keys: string[];
  group: DshHotkeyGroupId;
}

/** 目录基线：键位核对所用的 DSH 版本（DSH 升级复检后更新）。 */
export const BASELINE_DSH_VERSION = "0.1.5-rc.1";

/** 兼容核对过的 DSH 版本范围（展示用说明文字）。 */
export const BASELINE_VERIFIED_RANGE = "0.1.1-rc.2 ~ 0.1.5-rc.3";

export const DSH_HOTKEY_GROUPS: readonly DshHotkeyGroup[] = [
  { id: "composer", label: "输入框与发送" },
  { id: "trigger-menu", label: "/ 指令与 @ 引用菜单" },
  { id: "panels", label: "弹层与行内编辑" },
  { id: "trees", label: "树形与列表导航" },
];

export const DSH_HOTKEYS: readonly DshHotkeyEntry[] = [
  // 输入框与发送
  { id: "composer.send", label: "发送消息", keys: ["Enter"], group: "composer" },
  { id: "composer.newline", label: "输入框内换行", keys: ["Shift+Enter"], group: "composer" },
  { id: "composer.steer", label: "插话发送（智能体运行时）", keys: ["Ctrl/Cmd+Enter"], group: "composer" },
  { id: "composer.steer-queue", label: "插话发送全部排队消息（草稿为空且运行中）", keys: ["Ctrl/Cmd+Enter"], group: "composer" },
  { id: "composer.paste-files", label: "粘贴图片/文件进输入框", keys: ["Ctrl+V"], group: "composer" },
  { id: "composer.undo", label: "撤销", keys: ["Ctrl+Z"], group: "composer" },
  { id: "composer.redo", label: "重做", keys: ["Ctrl+Y", "Ctrl+Shift+Z"], group: "composer" },
  { id: "composer.delete-ref", label: "删除 @ 引用（整体移除）", keys: ["Backspace", "Delete"], group: "composer" },
  // / 指令与 @ 引用菜单
  { id: "menu.open-commands", label: "呼出指令菜单", keys: ["输入 /"], group: "trigger-menu" },
  { id: "menu.open-refs", label: "呼出文件/会话引用", keys: ["输入 @"], group: "trigger-menu" },
  { id: "menu.move", label: "高亮移动", keys: ["↑ / ↓"], group: "trigger-menu" },
  { id: "menu.drill", label: "进入子级目录", keys: ["Tab"], group: "trigger-menu" },
  { id: "menu.select", label: "选中高亮项", keys: ["Enter"], group: "trigger-menu" },
  { id: "menu.confirm", label: "确认补全（提交引用/指令）", keys: ["Space"], group: "trigger-menu" },
  { id: "menu.close", label: "关闭菜单", keys: ["Esc"], group: "trigger-menu" },
  // 弹层与行内编辑
  { id: "panels.close", label: "关闭弹层（设置/确认框/图片预览/会话统计/上下文用量）", keys: ["Esc"], group: "panels" },
  { id: "panels.menu-focus", label: "菜单焦点移动（移到首/末）", keys: ["↑ / ↓", "Home / End"], group: "panels" },
  { id: "panels.inline-edit", label: "行内编辑保存 / 取消（目标、排队消息、重命名）", keys: ["Enter", "Esc"], group: "panels" },
  // 树形与列表导航
  { id: "trees.expand", label: "展开 / 收起（子代理、JSON 树、轨迹分栏）", keys: ["→ / ←"], group: "trees" },
  { id: "trees.move", label: "焦点移动（含首/末）", keys: ["↑ / ↓", "Home / End"], group: "trees" },
];
