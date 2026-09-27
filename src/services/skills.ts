/** Tauri invoke 的类型化薄封装：Skill 中心（FR-14，T-4 定稿口径）。 */
import { invoke } from "./tauri";

export interface SkillCard {
  name: string;
  /** `owner/repo`。 */
  subName: string;
  description: string;
  iconUrl: string;
  stars: number;
  updatedAt: string;
  repo: string;
}

export interface SkillFeed {
  cards: SkillCard[];
  /** true = 来自本地离线快照（断网/配额降级）。 */
  offline: boolean;
  fetchedAt: string;
}

export interface SkillInstallOutcome {
  instanceId: string;
  /** 实际安装的技能名列表（一个仓库可含多个技能目录）。 */
  installed: string[];
  /** 落盘根目录：`<实例>/skills`。 */
  targetDir: string;
  overwritten: boolean;
}

export interface SkillProgress {
  instanceId: string;
  detail: string;
}



/** 技能目录信息流（GitHub `dsh-skill` 聚合；offline=true 时读离线快照）。 */
export function fetchSkillFeed(): Promise<SkillFeed> {
  return invoke<SkillFeed>("skill_feed");
}

/**
 * 安装一个技能仓库到实例（落点 `<实例>/skills/<name>/`，同名覆盖、幂等）。
 * 技能在实例下次启动时被 DSH 发现（自带目录 watcher），提示文案须写明。
 */
export function skillInstall(instanceId: string, repo: string): Promise<SkillInstallOutcome> {
  return invoke<SkillInstallOutcome>("skill_install", { id: instanceId, repo });
}

/** 订阅技能安装进度事件；返回取消订阅函数。 */
export async function onSkillProgress(
  handler: (progress: SkillProgress) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<SkillProgress>("skill-install-progress", (event) => {
    handler(event.payload);
  });
  return unlisten;
}
