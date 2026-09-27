/** Tauri invoke 的类型化薄封装：实例与注入配置相关命令。 */
import { invoke } from "./tauri";

export interface Instance {
  id: string;
  name: string;
  profile: string;
  nodeVersion: string | null;
  dshVersion: string | null;
  /** 实例专属固定 Web 端口（创建时分配，随名唯一，删号即灭）。 */
  port: number | null;
  lastLaunchedAt: string | null;
  createdAt: string;
  /** 外部实例（原地引用）：绝对路径；null = 托管实例（versions/<id>/）。 */
  dir: string | null;
}

/** 注入配置：settings 为 DSH settings.yaml 的 JSON 形态（顶层键=插件命名空间），env 为 .env 键值。 */
export interface InjectConfig {
  settings: Record<string, unknown>;
  env: Record<string, string>;
  /** 实例级 AGENTS.md 覆盖（版本设置页输入框）；缺省/null = 继承全局默认。 */
  agentsMd?: string | null;
}



/** 枚举全部实例（按创建顺序）。 */
export function listInstances(): Promise<Instance[]> {
  return invoke<Instance[]>("instance_list");
}

/** 实例根目录（instancesDir）——版本选择页展示每实例安装路径用。 */
export async function getInstancesDir(): Promise<string> {
  const info = await invoke<{ instancesDir: string }>("managed_root_info");
  return info.instancesDir;
}

/** 创建实例（名称 + 初始 profile），返回新实例元数据。 */
export function createInstance(name: string, profile: string): Promise<Instance> {
  return invoke<Instance>("instance_create", { name, profile });
}

/** 「添加已有文件夹」：把既有 DSH 实例目录原地注册为实例（数据留在原地）。 */
export function registerExternalInstance(folder: string): Promise<Instance> {
  return invoke<Instance>("instance_register_external", { folder });
}

/** 系统文件夹选择框；取消 = null。 */
export async function pickFolder(): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = (await open({
    directory: true,
    multiple: false,
    title: "选择要添加的实例文件夹（需含 instance.json 或已安装 DSH 本体）",
  })) as string | string[] | null;
  if (Array.isArray(picked)) return picked[0] ?? null;
  return picked;
}

/** 在文件管理器中打开实例子目录（skills / sessions / sources）。 */
export function openInstanceSubdir(id: string, which: "skills" | "sessions" | "sources"): Promise<string> {
  return invoke<string>("instance_open_subdir", { id, which });
}

/** 删除实例（整目录移除，零残留；运行中的实例会因文件占用而失败）。 */
export function deleteInstance(id: string): Promise<void> {
  return invoke<void>("instance_delete", { id });
}

/** 更新实例名称。 */
export function renameInstance(id: string, name: string): Promise<void> {
  return invoke<void>("instance_update", { id, name });
}

/** 读实例的注入覆盖；从未编辑过时为空配置。 */
export function getInjectOverride(id: string): Promise<InjectConfig> {
  return invoke<InjectConfig>("inject_get_override", { id });
}

/** 写实例的注入覆盖。 */
export function setInjectOverride(id: string, config: InjectConfig): Promise<void> {
  return invoke<void>("inject_set_override", { id, config });
}

/** 计算生效配置（全局默认 ⊕ 实例覆盖）。 */
export function getEffectiveInject(id: string): Promise<InjectConfig> {
  return invoke<InjectConfig>("inject_effective", { id });
}

/** 把生效配置写进实例目录（settings.yaml + .env），返回写入文件路径。 */
export function writeInstanceFiles(id: string): Promise<string[]> {
  return invoke<string[]>("inject_write_instance_files", { id });
}

/** 读全局默认注入模板。 */
export function getDefaultInject(): Promise<InjectConfig> {
  return invoke<InjectConfig>("inject_default_get");
}

/** 写全局默认注入模板。 */
export function setDefaultInject(config: InjectConfig): Promise<void> {
  return invoke<void>("inject_default_set", { config });
}

/** 读全局 AGENTS.md（null = 未设置；存储本体即托管根 AGENTS.md 文件，FILE-LAYOUT）。 */
export function getGlobalAgentsMd(): Promise<string | null> {
  return invoke<string | null>("inject_agents_global_get");
}

/** 写全局 AGENTS.md（null = 清除；实例下次配置物化时回落）。 */
export function setGlobalAgentsMd(content: string | null): Promise<void> {
  return invoke<void>("inject_agents_global_set", { content });
}
