/** 当前选中实例的进程内共享状态：启动页/版本选择页写入，二级页读取（刷新后回退启动页默认逻辑）。 */

let selectedInstanceId: string | null = null;

/** 记录当前选中实例 id（null = 未选择）。 */
export function setSelectedInstance(id: string | null): void {
  selectedInstanceId = id;
}

/** 读取当前选中实例 id；未选择时返回 null（调用方自行回退最近使用）。 */
export function getSelectedInstance(): string | null {
  return selectedInstanceId;
}
