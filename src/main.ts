import "./styles/theme.css";
import "./styles/main.css";
import { initTheme } from "./lib/theme";
import { initHintHost, showHint } from "./lib/hint";
import { createTopbar } from "./components/topbar";
import { launchPage } from "./routes/launch";
import { downloadsPage } from "./routes/downloads";
import { settingsPage } from "./routes/settings";
import { morePage } from "./routes/more";
import { instanceSelectPage } from "./routes/instance-select";
import { instanceSettingsPage } from "./routes/instance-settings";
import { marketDetailPage } from "./routes/market-detail";
import { tasksPage } from "./routes/tasks";
import { mountTaskFab } from "./components/task-fab";
import { mountRouter, registerPages } from "./router";
import { applyBackground } from "./lib/background";
import { isTempShowAll, setTempShowAll } from "./lib/uiPrefs";

// 首帧前套用已保存的自定义主题色（FR-11；无覆盖 = theme.css 静态默认蓝）。
initTheme();

// 个性化背景（FR-27）：每次启动随机加载 Pictures/ 里的一张（PCL 同款）。
void applyBackground();

// 功能隐藏的 F12 临时显示（PCL PageSetupUI.xaml:297：任意界面按 F12 暂时
// 显示被隐藏的功能）。preventDefault 防 WebView2 开 DevTools。
window.addEventListener("keydown", (event) => {
  if (event.key === "F12") {
    event.preventDefault();
    setTempShowAll(!isTempShowAll());
  }
});

registerPages([launchPage, downloadsPage, settingsPage, morePage, instanceSelectPage, instanceSettingsPage, marketDetailPage, tasksPage]);

const root = document.querySelector<HTMLElement>("#app");
if (!root) {
  throw new Error("#app 容器缺失");
}

root.classList.add("app-shell");
// 左下轻提示宿主（Phase 5 S5）：挂在 app-shell 容器内、弹窗遮罩层之下。
initHintHost(root);
mountTaskFab(root);
root.append(createTopbar());

const main = document.createElement("main");
main.className = "app-main";
root.append(main);

mountRouter(main);

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
  // 多文件拖入取第一个 .tgz（其余忽略）；非 tgz 仅防导航不提示（误拖是常态）。
  const file = Array.from(files).find((f) => /\.tgz$/i.test(f.name));
  if (!file) return;
  void (async () => {
    try {
      const [{ invoke }, { openModpackImport }] = await Promise.all([
        import("./services/tauri"),
        import("./routes/modpack-import"),
      ]);
      const bytes = new Uint8Array(await file.arrayBuffer());
      const stagedPath = await invoke<string>("modpack_stage_drop", {
        name: file.name,
        bytes,
      });
      await openModpackImport(stagedPath);
    } catch (err) {
      // err 已是统一层规范化后的中文友好句（对抗审查 R1 P1-05：不得裸透传原文）。
      showHint(`拖入导入失败：${String(err).slice(0, 240)}`);
    }
  })();
});
