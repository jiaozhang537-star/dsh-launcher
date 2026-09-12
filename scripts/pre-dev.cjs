/**
 * tauri dev 的前置脚本（tauri.conf.json beforeDevCommand）：
 * 先清掉占用 1420 的残留进程（只能是本项目失效的旧 vite/僵尸 node——
 * Windows 下关闭窗口偶尔不回收 npm 子进程，残留会占用端口导致下次
 * dev 起不来或界面无样式），再启动 vite 并转发退出码。
 */
const { execSync, spawn } = require("node:child_process");

const PORT = 1420;

function killStalePortHolder() {
  try {
    const out = execSync(
      "powershell -NoProfile -Command \"(Get-NetTCPConnection -LocalPort " +
        PORT +
        " -State Listen -ErrorAction SilentlyContinue).OwningProcess\"",
      { stdio: ["ignore", "pipe", "ignore"] },
    )
      .toString()
      .trim();
    const pids = [...new Set(out.split(/\s+/).filter(Boolean))];
    for (const pid of pids) {
      try {
        execSync(`taskkill /PID ${pid} /T /F`, { stdio: "ignore" });
        console.log(`[pre-dev] 已清理占用 ${PORT} 的残留进程 ${pid}`);
      } catch {
        /* 进程可能已自行退出 */
      }
    }
  } catch {
    /* 查询失败视同端口空闲 */
  }
}

killStalePortHolder();

const vite = spawn("npm", ["run", "dev"], { stdio: "inherit", shell: true });
vite.on("exit", (code) => process.exit(code ?? 0));
