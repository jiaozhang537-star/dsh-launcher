/** 热键持久化通道：hotkeys.json（随数据目录走，FILE-LAYOUT）。
 * 应用启动时水合一次（后端文件 → lib 覆盖缓存），并注册写通道；
 * 2026-09-12 前的 localStorage 绑定在此一次性迁移（迁完清除本地残留）。 */

import { invoke } from "@tauri-apps/api/core";

import { applyOverrides, setHotkeyPersistence, STORE_KEY } from "../lib/hotkeys";

/** 水合热键覆盖缓存并接通持久化通道。失败降级为全部默认键，不阻塞启动。 */
export async function hydrateHotkeys(): Promise<void> {
  let overrides: Record<string, string> = {};
  try {
    overrides = (await invoke<Record<string, string>>("hotkeys_get")) ?? {};
  } catch {
    overrides = {};
  }

  // 文件为空才看旧 localStorage（文件是权威；迁移成功才清除本地残留）。
  if (Object.keys(overrides).length === 0) {
    let legacy: Record<string, string> = {};
    try {
      legacy = JSON.parse(localStorage.getItem(STORE_KEY) ?? "{}") as Record<string, string>;
    } catch {
      legacy = {};
    }
    if (legacy && typeof legacy === "object" && Object.keys(legacy).length > 0) {
      let migrated = false;
      try {
        await invoke("hotkeys_set", { overrides: legacy });
        migrated = true;
      } catch {
        // 落盘失败不迁移：保留 localStorage，下次启动再试。
      }
      if (migrated) {
        overrides = legacy;
        localStorage.removeItem(STORE_KEY);
      }
    }
  }

  applyOverrides(overrides);
  setHotkeyPersistence((next) => {
    void invoke("hotkeys_set", { overrides: next }).catch(() => {
      // 落盘失败静默：缓存已是新值，下次改动会再尝试整表落盘。
    });
  });
}
