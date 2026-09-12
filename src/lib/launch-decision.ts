/** 启动页「打开网页」用的入口地址判定。 */

/** 网页入口地址的输入（与后端 ProcessState 的 webUrl/port 对齐）。 */
export interface WebEntryInput {
  /** `dsh web` 打印的带 token URL（后端从子进程 stdout 抓取；旧版本为 null）。 */
  webUrl: string | null;
  port: number | null;
}

/**
 * 算出要交给系统浏览器打开的地址。
 *
 * **优先用 `dsh web` 打印的 URL**：新版 DSH 的 web 面板带鉴权栅栏——根查询参数
 * 带进程 token，用它换 cookie 才放行；裸 `http://127.0.0.1:<port>/` 一律 401
 * （`dsh web authentication required; reopen the URL printed by dsh web.`）。
 * 旧版本不打印这行、也没有栅栏，裸端口 URL 才是它的正确入口。
 *
 * @param state - 进程状态里的入口信息
 * @returns 可打开的 URL；两者都缺失（无 URL 且无端口）时返回 null——此时宁可
 * 什么都不开，也不要造一个必然打不开的地址。
 */
export function webEntryUrl(state: WebEntryInput): string | null {
  const published = state.webUrl?.trim();
  if (published) return published;
  return state.port === null ? null : `http://127.0.0.1:${state.port}`;
}
