/** Tauri invoke 的类型化薄封装：插件市场信息流与版本枚举（FR-03，T-4 口径）。 */

export interface MarketCard {
  name: string;
  /** 副名：英文/仓库全名（PCL 卡片「钠 | Sodium」的后半）。 */
  subName: string;
  description: string;
  /** 方形图标 URL；空串 = 前端绘制字母块。 */
  iconUrl: string;
  tags: string[];
  /** npm 月下载（T-4 口径「下载人数」）；null = 降级隐藏。 */
  monthlyDownloads: number | null;
  /** npm 最新版本号；git 直装/dshhub 形态为 null。 */
  latestVersion: string | null;
  stars: number | null;
  updatedAt: string;
  source: "npm" | "github" | "dshhub";
  npmName: string | null;
  repo: string | null;
  category: string;
  verified: boolean | null;
}

/** 通道召回计数（0 结果空态的自证信息）。 */
export interface Recall {
  npm: number;
  github: number;
}

export interface MarketFeed {
  cards: MarketCard[];
  /** true = 来自本地离线快照（断网/配额降级），前端显著标注。 */
  offline: boolean;
  fetchedAt: string;
  recall: Recall;
}

export interface MarketVersion {
  version: string;
  latest: boolean;
  publishedAt: string;
  /** 每版本周下载（T-4：versions 端点仅周粒度）；null = 缺数据隐藏。 */
  weeklyDownloads: number | null;
}

/** 版本枚举应答：declaresBundle=false = 该包任一版本都未声明
 *  dsh.bundle.patch——是依赖库而非可加载插件（源头拦截）。 */
export interface VersionsReply {
  versions: MarketVersion[];
  declaresBundle: boolean;
}

export type MarketSort = "hot" | "new";
export type MarketSource = "all" | "npm" | "github" | "dshhub";

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(cmd, args);
}

/** 拉取插件市场信息流（排序/筛选在服务端应用；offline=true 时读离线快照）。 */
export function fetchMarketFeed(params: {
  sort?: MarketSort;
  keyword?: string;
  source?: MarketSource;
  kind?: string;
}): Promise<MarketFeed> {
  return invoke<MarketFeed>("market_feed", {
    sort: params.sort,
    keyword: params.keyword,
    source: params.source,
    kind: params.kind,
  });
}

/** 拉取 npm 包的历史版本与每版本周下载量（折叠卡片展开视图），
 *  同时回带是否声明插件层。 */
export function fetchMarketVersions(packageName: string): Promise<VersionsReply> {
  return invoke<VersionsReply>("market_versions", { package: packageName });
}

/** 整合包栏信息流（聚合型资源目录：全家桶/聚合包/套件；P3 浏览层）。 */
export function fetchModpackFeed(params: {
  sort?: MarketSort;
  keyword?: string;
  source?: MarketSource;
}): Promise<MarketFeed> {
  return invoke<MarketFeed>("modpack_feed", {
    sort: params.sort,
    keyword: params.keyword,
    source: params.source,
  });
}
