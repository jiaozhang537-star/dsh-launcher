/**
 * DSH 0.2.0 快捷键体系快照（热键页数据源）。
 *
 * 历史：P4.5（2026-09-25）曾定稿「DSH 键位全部硬编码、只读展示网页端键位」——
 * 该结论只考察过 0.1.x 的 web 源码；0.2.0-rc 线新增 dsh-client-shortcuts /
 * dsh-client-ui-shortcuts 快捷键系统（可编辑命令 + 固定动作，桌面壳存
 * userData/keybindings.json、web 存浏览器 localStorage dsh.keybindings.v1）。
 * 2026-10-04 需求方指令推倒重来，本表按 0.2.0-rc.2 重立。
 *
 * 口径：
 * - 默认值取 **web:windows 档**——启动器实例是 `dsh web`，浏览器里生效的就是
 *   web 档（桌面档默认不同，如左侧栏切换桌面 Ctrl+B、web Ctrl+Alt+B）；
 * - label 用官方 zh 词典原文；每条附安装版源码出处（相对
 *   `D:\dsh\桌面端\resources\dsh\node_modules\@deepseek-ai\`），随官方版本复检；
 * - 编辑不在启动器侧：web 键位存浏览器 localStorage，由实例网页的
 *   设置 → 快捷键 原生编辑器读写（启动器无文件通道，改了也不生效）。
 */

export type DshShortcutSection = "editable" | "fixed";

export interface DshShortcutEntry {
  /** 官方命令 id（0.2.0-rc.2 注册原文）。 */
  id: string;
  /** 官方中文名（各 ui-* 包 zh 词典原文）。 */
  label: string;
  /** 键帽文本；多数为单组合，双击/方向键为多条。 */
  keys: string[];
  section: DshShortcutSection;
}

/** 快捷键体系基线版本（随官方版本复检时才允许改动）。 */
export const BASELINE_DSH_SHORTCUTS_VERSION = "0.2.0-rc.2";

/** 页内小节：可编辑（官方设置页可改/可清/可恢复）在上，固定（registerFixed，不可改）在下。 */
export const DSH_SHORTCUT_SECTIONS: ReadonlyArray<{
  id: DshShortcutSection;
  label: string;
}> = [
  { id: "editable", label: "可编辑" },
  { id: "fixed", label: "固定" },
];

/** 官方 zh 词典原文的键位速查表（可编辑 18 + 固定 11 = 29 条）。 */
export const DSH_SHORTCUTS: readonly DshShortcutEntry[] = [
  // ===== 可编辑（ctx.shortcuts.register，web:windows 档默认值） =====
  // dsh-client-ui-layout/lib/client.js:628-655（web:windows :646-649）
  { id: "sidebar.left.toggle", label: "展开／收起左侧栏", keys: ["Ctrl+Alt+B"], section: "editable" },
  // dsh-client-ui-sidebar-right/lib/client.js:180-198
  { id: "sidebar.right.toggle", label: "展开／收起右侧栏", keys: ["Ctrl+Shift+B"], section: "editable" },
  // dsh-client-ui-sidebar-right/lib/client.js:233-238（Backslash + primary，desktop/web 同值）
  { id: "pane.split", label: "分栏", keys: ["Ctrl+\\"], section: "editable" },
  // dsh-client-ui-sidebar-right/lib/client.js:233-238（Enter + primary+alt）
  { id: "pane.fullscreen.toggle", label: "面板全屏／退出全屏", keys: ["Ctrl+Alt+Enter"], section: "editable" },
  // dsh-client-ui-sidebar-right/lib/client.js:281-293（web = desktop + alt）
  { id: "page.close", label: "关闭当前页面／窗口", keys: ["Ctrl+Alt+W"], section: "editable" },
  // dsh-client-ui-sidebar-right/lib/client.js:281-293（仅 web:windows 档绑定）
  { id: "page.refresh", label: "刷新当前页面", keys: ["Ctrl+Alt+R"], section: "editable" },
  // dsh-client-ui-sidebar-browser/lib/client.js:1548-1573（guide.title = "浏览器"）
  { id: "browser.new", label: "浏览器", keys: ["Ctrl+Alt+T"], section: "editable" },
  // dsh-client-ui-sidebar-files/lib/client.js:949-970（guide.title = "工作区文件"）
  { id: "workspace.files", label: "工作区文件", keys: ["Ctrl+Alt+P"], section: "editable" },
  // dsh-client-ui-sidebar-terminal/lib/client.js:388-408（Backquote + control）
  { id: "terminal.new", label: "新建终端", keys: ["Ctrl+`"], section: "editable" },
  // dsh-client-ui-open-in-app/lib/client.js:777-796（web:windows :790-794）
  { id: "workspace.openLocal", label: "在本地打开", keys: ["Ctrl+Shift+O"], section: "editable" },
  // dsh-client-ui-settings-general/lib/client.js:1065-1086（Comma + primary+alt）
  { id: "settings.open", label: "打开设置", keys: ["Ctrl+Alt+,"], section: "editable" },
  // dsh-client-ui-shortcuts/lib/client.js:1093-1118（六档全同 Slash + primary）
  { id: "shortcuts.open", label: "快捷键速查", keys: ["Ctrl+/"], section: "editable" },
  // dsh-client-ui-workspace/lib/client.js:108-143（register 循环：desktop primary / web primary+alt）
  { id: "session.new", label: "新会话", keys: ["Ctrl+Alt+N"], section: "editable" },
  // dsh-client-ui-workspace/lib/client.js:146
  { id: "session.search", label: "搜索会话", keys: ["Ctrl+Alt+K"], section: "editable" },
  // dsh-client-ui-workspace/lib/client.js:150
  { id: "workspace.add", label: "添加工作区", keys: ["Ctrl+Alt+O"], section: "editable" },
  // dsh-client-ui-workspace/lib/client.js:160（desktop/web 同为 primary+alt）
  { id: "session.rename", label: "重命名会话", keys: ["Ctrl+Alt+G"], section: "editable" },
  // dsh-client-ui-workspace/lib/client.js:172（web modifiers = ["primary","shift"]）
  { id: "session.fork", label: "分叉会话", keys: ["Ctrl+Shift+F"], section: "editable" },
  // dsh-client-ui-workspace/lib/client.js:193（web modifiers = ["primary","alt"]）
  { id: "session.archive", label: "归档会话", keys: ["Ctrl+Alt+A"], section: "editable" },
  // ===== 固定（registerFixed，runtime 无关，不可改） =====
  // dsh-client-ui-conversation/lib/client.js:18032-18037（input.send）
  { id: "fixed.send", label: "发送消息", keys: ["Enter"], section: "fixed" },
  // dsh-client-ui-conversation/lib/client.js:18038-18043（shortcut.newline）
  { id: "fixed.newline", label: "换行", keys: ["Shift+Enter"], section: "fixed" },
  // dsh-client-ui-conversation/lib/client.js:18045-18052（shortcut.complementary）
  { id: "fixed.complementary", label: "使用互补的 Queue／Steer 方式", keys: ["Ctrl+Enter"], section: "fixed" },
  // dsh-client-ui-conversation/lib/client.js:18052-18057（shortcut.slash）
  { id: "fixed.slash", label: "打开命令菜单", keys: ["/"], section: "fixed" },
  // dsh-client-ui-conversation/lib/client.js:18058-18063（shortcut.mention）
  { id: "fixed.mention", label: "打开引用菜单", keys: ["Shift+2"], section: "fixed" },
  // dsh-client-ui-conversation/lib/client.js:18090-18100（input.stop；双击 Esc，间隔上限 500ms）
  { id: "response.stop", label: "停止生成", keys: ["Esc", "Esc"], section: "fixed" },
  // dsh-client-ui-approval/lib/client.js:322-331（allowOnce）
  { id: "approval.allow", label: "允许一次", keys: ["Enter"], section: "fixed" },
  // dsh-client-ui-approval/lib/client.js:332-341（reject）
  { id: "approval.reject", label: "拒绝", keys: ["Esc"], section: "fixed" },
  // dsh-client-ui-shortcuts/lib/client.js:1006-1015
  { id: "fixed.move", label: "移动菜单选择", keys: ["↑", "↓"], section: "fixed" },
  // dsh-client-ui-shortcuts/lib/client.js:1016-1023
  { id: "fixed.select", label: "选择菜单项", keys: ["Enter"], section: "fixed" },
  // dsh-client-ui-shortcuts/lib/client.js:1030-1037
  { id: "fixed.dismiss", label: "关闭菜单或顶层弹窗", keys: ["Esc"], section: "fixed" },
];
