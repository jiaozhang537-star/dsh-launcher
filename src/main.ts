import "./styles/theme.css";
import "./styles/main.css";
import { createTopbar } from "./components/topbar";
import { launchPage } from "./routes/launch";
import { downloadsPage } from "./routes/downloads";
import { settingsPage } from "./routes/settings";
import { morePage } from "./routes/more";
import { instanceSelectPage } from "./routes/instance-select";
import { instanceSettingsPage } from "./routes/instance-settings";
import { mountRouter, navigate, registerPages } from "./router";
import { comboMatches, HOTKEY_ACTIONS, isTypingTarget, loadBindings } from "./lib/hotkeys";
import { hydrateHotkeys } from "./services/hotkeys";

registerPages([launchPage, downloadsPage, settingsPage, morePage, instanceSelectPage, instanceSettingsPage]);

const root = document.querySelector<HTMLElement>("#app");
if (!root) {
  throw new Error("#app 容器缺失");
}

root.classList.add("app-shell");
root.append(createTopbar());

const main = document.createElement("main");
main.className = "app-main";
root.append(main);

mountRouter(main);

// 热键覆盖从 hotkeys.json 水合（随数据目录走，FILE-LAYOUT）；失败降级为默认键。
void hydrateHotkeys();

// 窗口已关闭 Tauri 原生拖放（dragDropEnabled=false，否则 WebView2 会吞掉
// 页面内的 HTML5 拖拽——提供方卡片拖拽排序实测失效）。此处全局拦截
// dragover/drop：一是防误拖文件导致 WebView 导航走丢，二是 P4 的
// 「整合包文件拖入」——任何位置拖入 .tgz 都触发导入流程（其余文件仅防导航）。
// WebView 的 File 对象拿不到真实路径（原生拖放已禁用），走内容通道：
// 读字节 → modpack_stage_drop 落临时文件 → 以该路径进导入流程。
window.addEventListener("dragover", (event) => event.preventDefault());
window.addEventListener("drop", (event) => {
  event.preventDefault();
  const files = event.dataTransfer?.files;
  if (!files || files.length === 0) return;
  const file = files[0];
  if (!/\.tgz$/i.test(file.name)) return;
  void (async () => {
    try {
      const [{ invoke }, { openModpackImport }] = await Promise.all([
        import("@tauri-apps/api/core"),
        import("./routes/modpack-import"),
      ]);
      const bytes = new Uint8Array(await file.arrayBuffer());
      const stagedPath = await invoke<string>("modpack_stage_drop", {
        name: file.name,
        bytes,
      });
      await openModpackImport(stagedPath);
    } catch (err) {
      alert(`拖入导入失败：${String(err).slice(0, 240)}`);
    }
  })();
});

// 应用内快捷键（热键页可视化配置）：输入类控件聚焦时不触发。
window.addEventListener("keydown", (event) => {
  if (isTypingTarget(event.target)) return;
  for (const binding of loadBindings()) {
    if (!comboMatches(event, binding.combo)) continue;
    const target = HOTKEY_ACTIONS.find((action) => action.id === binding.action)?.target;
    if (target) {
      event.preventDefault();
      navigate(target);
    }
    return;
  }
});
