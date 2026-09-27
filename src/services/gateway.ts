/** Tauri invoke 的类型化薄封装：统一下载网关与镜像源设置（FR-18）。 */
import { invoke } from "./tauri";

export interface MirrorSourceInfo {
  id: string;
  label: string;
}

export interface MirrorSettingsInfo {
  active: string;
  customPrefix: string;
  /** 社区资源（插件 / Skill）通道激活源；Phase 3 起被消费。 */
  communityActive: string;
  sources: MirrorSourceInfo[];
  communitySources: MirrorSourceInfo[];
}

export interface ProbeResult {
  ok: boolean;
  latencyMs: number;
  detail: string;
}



/** 读镜像源设置（含版本通道与社区通道的内置源清单）。 */
export function getMirror(): Promise<MirrorSettingsInfo> {
  return invoke<MirrorSettingsInfo>("mirror_get");
}

/**
 * 保存版本下载通道设置：激活源 id + 自定义前缀（custom 时必填合法前缀）。
 * 只更新提供的字段，社区通道设置不受影响。
 */
export function setMirror(active: string, prefix: string): Promise<void> {
  return invoke<void>("mirror_set", { active, prefix });
}

/** 保存社区资源镜像源设置（只更新该字段；Phase 3 起被消费）。 */
export function setCommunityMirror(source: string): Promise<void> {
  return invoke<void>("mirror_set", { communityActive: source });
}

/**
 * 测试指定源连通性；不传 source 时测版本通道当前激活源。
 * 版本卡传 "official"（npm 镜像 + GitHub 双探测点）、社区卡传 "github" / "gitee" / "dshhub"。
 */
export function testMirror(source?: string, prefix?: string): Promise<ProbeResult> {
  return invoke<ProbeResult>("mirror_test", { source, prefix });
}
