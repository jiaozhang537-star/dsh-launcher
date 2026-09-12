/** 当前选中实例的进程内共享状态：启动页/版本选择页写入，二级页读取（刷新后回退启动页默认逻辑）。 */

let selectedInstanceId: string | null = null;
let selectedInstanceName: string | null = null;

/**
 * 记录当前选中实例 id 与名称（null = 未选择，名称一并清除）。
 * 名称供「版本设置」二级页顶栏标题同步求值——曾因标题读异步回填的字段，
 * 内容区已显示实例而顶栏仍写「未选择实例」。
 */
export function setSelectedInstance(id: string | null, name?: string | null): void {
  selectedInstanceId = id;
  if (id === null) {
    selectedInstanceName = null;
  } else if (name !== undefined) {
    selectedInstanceName = name ?? null;
  }
}

/** 读取当前选中实例 id；未选择时返回 null（调用方自行回退最近使用）。 */
export function getSelectedInstance(): string | null {
  return selectedInstanceId;
}

/** 读取当前选中实例名称；未知或未选择返回 null。 */
export function getSelectedInstanceName(): string | null {
  return selectedInstanceName;
}
