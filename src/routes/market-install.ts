/**
 * 市场安装共享层（Phase 5 返工 W）：插件/整合包两份逐字同构的安装链路去重上移，
 * 供列表页与详情二级页共用。安装语义与回执文案保持等价（聚合包进度文案多一句，
 * 由 kind 参数区分）；状态与版本缓存上移到模块级——切分类/切页不再丢失。
 */

import { listInstances } from "../services/instances";
import { getSelectedInstance } from "../lib/selected-instance";
import { sortInstancesByRecency } from "../lib/instanceOrder";
import { pluginInstall } from "../services/plugins";
import {
  fetchGithubReleases,
  fetchMarketVersions,
  type GithubRelease,
  type MarketCard,
  type VersionsReply,
} from "../services/market";
import type { Instance } from "../services/instances";
import { upsertTask, updateTask } from "../lib/downloadTasks";
import { navigate } from "../router";
import { showHint } from "../lib/hint";

export type InstallPhase = "installing" | "done" | "failed";

export interface InstallState {
  phase: InstallPhase;
  detail: string;
}

/** 安装状态（spec → 状态）。模块级：列表页与详情页共享。 */
export const installStates = new Map<string, InstallState>();

/** npm 版本枚举缓存（包名 → 应答）。模块级：列表页与详情页共享。 */
export const versionCache = new Map<string, VersionsReply>();

/** GitHub Releases 缓存（repo → 列表）。模块级：详情页共用。 */
export const releaseCache = new Map<string, GithubRelease[]>();

/** 带缓存的 Releases 枚举（git 直装卡的「历代版本」）。 */
export function fetchReleasesCached(repo: string): Promise<GithubRelease[]> {
  const cached = releaseCache.get(repo);
  if (cached) return Promise.resolve(cached);
  return fetchGithubReleases(repo).then((releases) => {
    releaseCache.set(repo, releases);
    return releases;
  });
}

/** 带缓存的版本枚举。 */
export function fetchVersionsCached(packageName: string): Promise<VersionsReply> {
  const cached = versionCache.get(packageName);
  if (cached) return Promise.resolve(cached);
  return fetchMarketVersions(packageName).then((reply) => {
    versionCache.set(packageName, reply);
    return reply;
  });
}

/** 安装 spec 规则（契约）：npm 包 `name@version`；git 卡不带版本 = `github:owner/repo`
 *  （默认分支），带版本（Release tag）= `github:owner/repo#tag`——后端原样透传
 *  给 DSH CLI/pnpm（# 后即 tag/分支），重复安装兜底已按 # 前段取包名。 */
export function installSpecOf(card: MarketCard, version: string | null): string {
  if (!version) return `github:${card.repo}`;
  return card.npmName ? `${card.npmName}@${version}` : `github:${card.repo}#${version}`;
}

/** 当前实例解析：显式选择 > 最近使用（launch 页同口径）> 唯一实例。
 *  内存单例重启即空——不能因此拒绝安装（走查实证的误报场景）。 */
export async function resolveCurrentInstance(): Promise<Instance | null> {
  const instances = await listInstances();
  if (instances.length === 0) return null;
  const selectedId = getSelectedInstance();
  if (selectedId) {
    const picked = instances.find((i) => i.id === selectedId);
    if (picked) return picked;
  }
  return sortInstancesByRecency(instances)[0] ?? null;
}

/** 点下载图标直接装进**当前实例**（不弹选择框）；「启用」= 装入 profile +
 *  bundles 层（DSH 原生无启停开关，T-2：下次启动实例即生效）。
 *  statusLine 打 dataset.spec：downloads 页全局进度行按它各归各（审查 R1）——
 *  进度落在发起安装的页面行上（列表页或详情页各自持有自己的 statusLine）。 */
export function installToCurrent(
  kind: "plugin" | "modpack",
  card: MarketCard,
  version: string | null,
  statusLine: HTMLElement,
  onSettled: () => void,
): void {
  const spec = installSpecOf(card, version);
  // 任务体系接入（H3）：市场安装与 dsh 安装共用任务 store，任务管理页/右下
  // 浮窗都能看到；聚合包安装 = PCL Install_Click 原式，发起即切任务管理页。
  upsertTask({
    instanceId: spec,
    name: `${card.name}${version ? ` ${version}` : ""}`,
    version: version ?? "",
    status: "running",
    phase: "install",
    detail: "",
    received: 0,
    total: 0,
    speed: null,
    startedAt: Date.now(),
  });
  statusLine.classList.remove("mk-status--error");
  statusLine.textContent = "正在确定目标实例…";
  void (async () => {
    const inst = await resolveCurrentInstance();
    if (!inst) {
      statusLine.classList.add("mk-status--error");
      statusLine.textContent = "还没有任何实例——先到「下载 · dsh 版本」创建一个，再回来安装。";
      return;
    }
    installStates.set(spec, { phase: "installing", detail: "" });
    onSettled();
    if (kind === "modpack") navigate("tasks");
    statusLine.dataset.spec = spec;
    statusLine.textContent =
      `正在安装 ${spec} 到「${inst.name}」…` +
      (kind === "modpack" ? "（聚合包会一并装齐它声明的全部插件）" : "");
    try {
      const outcome = await pluginInstall(inst.id, spec);
      installStates.set(spec, { phase: "done", detail: outcome.version });
      updateTask(spec, {
        status: "done",
        phase: "done",
        detail: `${outcome.packageName} ${outcome.version} → 「${inst.name}」`,
      });
      showHint(`${outcome.packageName} 已装进「${inst.name}」`);
      statusLine.textContent = "";
    } catch (err) {
      installStates.set(spec, { phase: "failed", detail: String(err).slice(0, 160) });
      updateTask(spec, { status: "failed", phase: "failed", detail: String(err).slice(0, 400) });
      statusLine.classList.add("mk-status--error");
      statusLine.textContent = `安装失败：${String(err).slice(0, 260)}`;
    }
    onSettled();
  })();
}
