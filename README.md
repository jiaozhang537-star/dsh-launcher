# DSH 启动器

DeepSeek Harness（DSH）的图形化启动器：**像 PCL 管理 Minecraft 一样管理你的 DSH**——多实例下载与全套隔离、插件市场、Skill 中心、整合包分享、运行环境（Node）托管、模型厂商配置。

**它只是壳**：不修改 DSH 本体一行代码，一切设置都通过 DSH 的原生配置机制（实例目录下的 `settings.yaml` / `.env` / 进程环境变量）生效。

## 下载与安装

**系统要求**：Windows 10（1809+）或 Windows 11，64 位；WebView2 运行库缺失时安装包会自动引导安装（无需手动下载）。

1. 到 [GitHub Releases](https://github.com/jiaozhang537-star/dsh-launcher/releases) 下载 `dsh-launcher_0.1.0_x64-setup.exe`；
2. 双击运行。首次运行若出现「**Windows 已保护你的电脑**」（SmartScreen），点 **更多信息 → 仍要运行**——安装包未做代码签名，这是预期提示；
3. 按向导完成安装（当前用户安装，**不需要管理员权限**），开始菜单出现「DSH 启动器」。

## 四步上手

1. **配 Key**：打开「设置」页，勾选模型厂商（如 DeepSeek）并填入 API Key——Key 以 Windows 凭据保护（DPAPI）密文落盘，明文不落任何文件；
2. **装实例**：到「下载」页选 DSH 版本创建实例（缺 Node 会自动代管下载），任务卡实时显示进度；
3. **启动**：回主页选厂商与默认模型，点「启动」——校验链、对账、拉起全程字幕可见；
4. **进会话**：网页就绪后自动用系统浏览器打开 DSH；装插件去「下载 · 插件」市场，搬配置用「整合包」导出/拖入。

同一份四步指引在应用内「更多 · 使用帮助」。

## 数据位置与卸载

启动器有两处数据目录：

| 目录 | 内容 |
| --- | --- |
| `%LOCALAPPDATA%\dsh-launcher` | 托管根：全部实例（versions/）、代管 Node（nodes/）、凭据库（keyvault.json）、市场缓存、共享区（shared/） |
| `%APPDATA%\com.mltz.dshlauncher` | WebView2 界面数据（窗口状态等） |

**卸载**时会出现数据警示卡，列出上述目录与当前占用，由你选择：

- **否（默认）= 保留数据**：应用被移除，两处数据目录保留——重装后实例与配置原样还在；
- **是 = 一并删除**：两处数据目录彻底清空，重装后为全新状态。

卸载前建议先在启动器里停止运行中的实例。除以上两处外，卸载不留注册表残留、无服务、无自启。

## 镜像源与 GitHub Token

- 国内直连 GitHub 不稳定时，在「设置 · 镜像源」里可切换 npmmirror 等公开镜像（npm/Node 下载默认即镜像优先、官方兜底），或填自定义 GitHub 前缀；连通性可一键测试；
- GitHub 匿名请求限流（60 次/小时）会限制市场列表刷新——在「设置」里填一枚 GitHub Token 可提额；Token 与 API Key 一样走 DPAPI 密文存储；
- 不想用镜像？所有通道都回落官方源。

## 第三方插件与凭据安全

- 插件市场聚合的是 npm / GitHub 上的**第三方公开插件**：安装走 DSH 官方安装链，但其代码质量与行为由其作者负责，装前请自行判断；整合包来自他人分享时同理；
- 你的 API Key 只存本机（DPAPI 加密），不会进入整合包（导出时明确排除）、不会上传任何服务器——本项目无自建后端，数据聚合全部走 GitHub 与公共镜像源的公开接口；
- 源码与安装包的第三方组件许可清单见 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)。

## 从源码构建

前置：Node ≥ 22、Rust 工具链、Tauri 2 的 Windows 依赖（WebView2 + MSVC 构建工具）。

```sh
npm install
npm run tauri dev      # 开发运行（前端热更 + Rust 增量编译）
npm run tauri build    # 出 Windows 安装包（NSIS，含卸载数据警示卡）
```

门禁（提交前必须全绿）：

```sh
cd src-tauri && cargo test --lib          # Rust 单测
npm run test                              # 前端单测（vitest）
npm run build                             # tsc 类型检查 + vite 打包
cd src-tauri && cargo test --lib -- --ignored   # 走网络 / 需本机实例的 e2e
```

## 目录

```text
src/                 前端（Vanilla TS + 自研极简路由）
  routes/            页面：启动 / 下载 / 版本设置 / 设置 / 更多
  services/          Tauri invoke 的类型化封装（每个命令一处）
  lib/               纯函数逻辑（可 vitest 直测）
src-tauri/src/       后端（Rust）
  node_runtime.rs    托管 Node：下载、解包、选版
  dsh_install.rs     DSH 本体安装（npm）
  source_archive.rs  源码包安装
  instances.rs       实例注册表与目录布局
  inject.rs          注入配置的合并与落盘（settings.yaml / .env）
  llm_bridge.rs      厂商配置 → 实例 llm-pi-ai 路由 / 默认模型的桥接
  keyvault.rs        DPAPI 密文保险库 + 环境变量名（注入与路由共用）
  precheck.rs        启动前校验链（Node / Key / 路由 / 默认模型 / 完整性 / 插件）
  process.rs         实例进程生命周期（拉起、钉端口、运行记录、停）
  self_update.rs     启动器自更新（GitHub Releases + SHA256 校验）
  log_sentinel.rs    DSH 输出 → 人话诊断
  market.rs         插件市场聚合（GitHub + npm + 镜像网关）
  plugin_manage.rs  插件清单与增删（复用 DSH 官方 add/remove 链）
  skills.rs         Skill 安装
  modpack.rs        整合包导出与导入
src-tauri/windows/   NSIS 安装器钩子（卸载数据警示卡）
```

## 变更记录

见 [CHANGELOG.md](CHANGELOG.md)。

## 许可

MIT（见 [LICENSE](LICENSE)）——仅需保留署名，允许商用与二次分发。
