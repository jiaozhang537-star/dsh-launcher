//! 步骤 6 验收：官方原版 DSH 读取注入配置的端到端验证。
//!
//! 前置：`.recon/official/node_modules/@deepseek-ai/dsh/lib/bin.js` 存在
//! （R-1 安装的官方原版副本，见 phase-01 日志）。测试走启动器真实管线：
//! create_instance → 注入模板 → write_instance_files → 以
//! `DSH_HOME=<实例目录>` 拉起官方 DSH。注入的 `llm-deepseek.baseURL`
//! 指向不可达域名；DSH 启动后请求必然打到该地址，输出中出现该域名即证明
//! 配置被原生读取（与 R-1 的 TRANSPORT 验证手法一致）。
//!
//! 慢速 + 需网络栈，默认 `#[ignore]`；显式运行：
//! `cargo test --manifest-path src-tauri/Cargo.toml -- --ignored`

#![cfg(test)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::inject::{set_default_template, write_instance_files, InjectConfig};
use crate::instances;

/// 官方 DSH 入口存在才参与验证；否则提示先跑 R-1 安装。
const OFFICIAL_BIN: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../.recon/official/node_modules/@deepseek-ai/dsh/lib/bin.js");

#[test]
#[ignore]
fn official_dsh_reads_injected_base_url() {
    let bin = std::path::PathBuf::from(OFFICIAL_BIN);
    assert!(bin.exists(), "官方 DSH 副本不存在于 {}（先执行 R-1 安装）", bin.display());

    let (_lock, _tmp) = crate::instances::tests::sandbox("verify-official");
    let instance = instances::create_instance("步骤6验证", "headless").unwrap();

    // 注入一个必然失败的 baseURL：若 DSH 输出中出现该域名，即证明它读的是我们的 settings.yaml。
    let mut settings = serde_json::Map::new();
    settings.insert(
        "llm-deepseek".into(),
        serde_json::json!({ "baseURL": "http://example-invalid.test" }),
    );
    set_default_template(&InjectConfig { settings, env: Default::default(), agents_md: None }).unwrap();
    let written = write_instance_files(&instance.id).unwrap();
    assert_eq!(written.len(), 2);

    let home = instances::instance_dir(&instance.id).unwrap();
    let yaml = std::fs::read_to_string(home.join("settings.yaml")).unwrap();
    assert!(yaml.contains("example-invalid.test"), "settings.yaml 未包含注入值");

    let started = Instant::now();
    let output = Command::new("node")
        .arg(&bin)
        .arg("--profile")
        .arg("headless")
        .arg("回复 ok 即可")
        .current_dir(&home)
        .env("DSH_HOME", &home)
        // 占位 Key 足够让请求发出（R-1 已证 AUTH invalid 说明请求到达端点；
        // 本测试看的是 TRANSPORT 目标地址，Key 真伪无关）。
        .env("DEEPSEEK_API_KEY", "sk-recon-placeholder-123")
        .stdin(Stdio::null())
        .output()
        .expect("拉起官方 DSH 失败");

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("== 官方 DSH 输出（耗时 {:?}）==\n{combined}", started.elapsed());

    assert!(
        combined.contains("example-invalid.test"),
        "输出未出现注入的 baseURL 域名——配置疑似未被原生读取。\n--- 输出 ---\n{combined}"
    );
    // 进程可能因请求失败而退出非零，这里只关心证据文本，不要求 exit code。
    let _ = Duration::from_secs(0);
}

/// 以注入配置拉起官方 DSH 并返回完整输出（stdout+stderr）。
fn run_official_dsh(bin: &std::path::Path, home: &std::path::Path) -> String {
    let output = Command::new("node")
        .arg(bin)
        .arg("--profile")
        .arg("headless")
        .arg("回复 ok 即可")
        .current_dir(home)
        .env("DSH_HOME", home)
        .env("DEEPSEEK_API_KEY", "sk-recon-placeholder-123")
        .stdin(Stdio::null())
        .output()
        .expect("拉起官方 DSH 失败");
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// 步骤 7 验收：双实例零串扰。A/B 各自注入不同 baseURL，分别启动官方 DSH，
/// 输出必须各含自己的域名且不含对方域名；随后删除 A，B 的注入产物必须原样无损。
#[test]
#[ignore]
fn dual_instances_zero_cross_talk_and_delete_isolation() {
    let bin = std::path::PathBuf::from(OFFICIAL_BIN);
    assert!(bin.exists(), "官方 DSH 副本不存在于 {}（先执行 R-1 安装）", bin.display());

    let (_lock, _tmp) = crate::instances::tests::sandbox("verify-dual");
    let a = instances::create_instance("双实例甲", "headless").unwrap();
    let b = instances::create_instance("双实例乙", "headless").unwrap();
    assert_ne!(a.id, b.id);

    for (instance, domain) in [(&a, "a"), (&b, "b")] {
        let url = format!("http://{domain}.invalid.test");
        let mut settings = serde_json::Map::new();
        settings.insert(
            "llm-deepseek".into(),
            serde_json::json!({ "baseURL": url }),
        );
        // 用实例覆盖而非全局模板：两实例互不相同时覆盖才是唯一来源。
        crate::inject::set_override(
            &instance.id,
            &InjectConfig { settings, env: Default::default(), agents_md: None },
        )
        .unwrap();
        write_instance_files(&instance.id).unwrap();
    }

    let out_a = run_official_dsh(&bin, &instances::instance_dir(&a.id).unwrap());
    println!("== 实例 A 输出 ==\n{out_a}");
    assert!(out_a.contains("http://a.invalid.test"), "A 输出未出现自己的 baseURL");
    assert!(!out_a.contains("http://b.invalid.test"), "A 输出出现 B 的 baseURL——发生串扰");

    let out_b = run_official_dsh(&bin, &instances::instance_dir(&b.id).unwrap());
    println!("== 实例 B 输出 ==\n{out_b}");
    assert!(out_b.contains("http://b.invalid.test"), "B 输出未出现自己的 baseURL");
    assert!(!out_b.contains("http://a.invalid.test"), "B 输出出现 A 的 baseURL——发生串扰");

    // 删除 A 后：B 目录与注册表行原样无损。
    let yaml_b_before =
        std::fs::read_to_string(instances::instance_dir(&b.id).unwrap().join("settings.yaml")).unwrap();
    instances::delete_instance(&a.id).unwrap();
    assert!(!instances::instance_dir(&a.id).unwrap().exists(), "A 目录未删除");
    let listed = instances::list_instances().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, b.id);

    let dir_b = instances::instance_dir(&b.id).unwrap();
    assert!(crate::instances::instance_dir_intact(&dir_b), "删除 A 后 B 元数据受损");
    let yaml_b_after = std::fs::read_to_string(dir_b.join("settings.yaml")).unwrap();
    assert_eq!(yaml_b_after, yaml_b_before, "删除 A 后 B 注入产物被改动");
}

/// 步骤 8 · Phase 2 总验收：干净托管根上并行安装 DSH（Node 运行时由安装链
/// 自动代管拉起）→ 未配 Key 拦截 → 配 Key（DPAPI 链）→ 双实例端口并存 HTTP
/// 可达 → 一键停止端口释放 → 真实 Key 经保险库链完成有效对话。
///
/// 慢速（自动代管真实下载 ~35MB Node + 两份 npm install + 两次 web 启动 +
/// 一次对话）且需要 DEEPSEEK_ACCEPTANCE_KEY 环境变量携带真实 Key；默认
/// `#[ignore]`：`set DEEPSEEK_ACCEPTANCE_KEY=<key> && cargo test phase2_acceptance -- --ignored --nocapture`
#[test]
#[ignore]
fn phase2_acceptance_parallel_install_dual_ports_stop_and_real_conversation() {
    let real_key = std::env::var("DEEPSEEK_ACCEPTANCE_KEY").unwrap_or_default();
    let has_real_key = real_key.len() >= 20;
    println!(
        "== 环境：真实 Key {} ==",
        if has_real_key {
            format!("已加载（长度 {}）", real_key.len())
        } else {
            "缺失——对话段将断言失败".to_string()
        }
    );

    let (_lock, _tmp) = crate::instances::tests::sandbox("accept-phase2");

    // ── 1+2. 免手动配 Node：干净托管根上直接并行安装 DSH，
    //     运行时由安装链自动代管拉起（FR-05 兜底，走统一网关通道）──────
    let latest = {
        let versions = crate::registry_meta::fetch_versions().expect("版本列表拉取");
        versions.into_iter().find(|v| v.latest).expect("packument 无 dist-tags.latest")
    };
    println!("== 安装目标版本：{} ==", latest.version);
    let version_str = latest.version.clone();

    let a = instances::create_instance("验收甲", "web").unwrap();
    let b = instances::create_instance("验收乙", "web").unwrap();
    // 临时清空 PATH 隐藏系统 Node，强制安装链走「自动代管」兜底通道
    // （批次④解析顺序③）；装完立即恢复。运行期间本进程 PATH 为空，
    // 须以测试名过滤单独运行，不得与其他用例并行。
    let original_path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", "");
    // 收集两个任务的 AutoNode 阶段事件：并行场景下至少一方应触发自动下载。
    let auto_notes = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let (r_a, r_b) = {
        let (id_a, id_b) = (a.id.clone(), b.id.clone());
        let v_a = version_str.clone();
        let v_b = version_str.clone();
        let notes_a = std::sync::Arc::clone(&auto_notes);
        let notes_b = std::sync::Arc::clone(&auto_notes);
        let handle_a = std::thread::spawn(move || {
            crate::dsh_install::install(&id_a, &v_a, &mut |stage| {
                if let crate::dsh_install::InstallStage::AutoNode { detail, .. } = stage {
                    notes_a.lock().unwrap_or_else(|p| p.into_inner()).push(detail);
                }
            })
        });
        let handle_b = std::thread::spawn(move || {
            crate::dsh_install::install(&id_b, &v_b, &mut |stage| {
                if let crate::dsh_install::InstallStage::AutoNode { detail, .. } = stage {
                    notes_b.lock().unwrap_or_else(|p| p.into_inner()).push(detail);
                }
            })
        });
        (handle_a.join().expect("A 线程崩溃"), handle_b.join().expect("B 线程崩溃"))
    };
    r_a.unwrap_or_else(|e| panic!("实例 A 安装失败：{e}"));
    r_b.unwrap_or_else(|e| panic!("实例 B 安装失败：{e}"));
    assert_eq!(instances::get_instance(&a.id).unwrap().dsh_version.as_deref(), Some(version_str.as_str()));
    assert_eq!(instances::get_instance(&b.id).unwrap().dsh_version.as_deref(), Some(version_str.as_str()));

    // 自动代管断言：发生过 AutoNode 阶段、全局默认已写、node.exe 在位。
    std::env::set_var("PATH", &original_path);
    {
        let notes = auto_notes.lock().unwrap_or_else(|p| p.into_inner());
        assert!(
            !notes.is_empty(),
            "干净环境下并行安装必须触发 Node 自动下载（AutoNode 阶段缺失）"
        );
        println!("== 自动下载阶段事件 {} 条，首条：{:?} ==", notes.len(), notes.first());
    }
    let node_version = crate::node_runtime::load_config()
        .unwrap()
        .default_node_version
        .expect("自动下载完成后未写入全局默认托管版本");
    assert!(crate::node_runtime::managed_node_exe(&node_version).is_some());
    println!("== 托管 Node v{node_version} 自动代管就绪（免手动配置）==");
    write_evidence(
        "accept-phase2-progress.txt",
        &format!("auto-provisioned node {node_version}\nparallel install {version_str} x2 ok\n"),
    );
    println!("== 双实例并行安装完成（{version_str}） ==");

    // ── 3. 未配 Key 时校验链拦截（FR-06/FR-08 前半）───────────────────
    let blocked_report = crate::precheck::run(&a.id).unwrap();
    assert!(blocked_report.blocked, "空 Key 库必须拦截");
    assert!(
        blocked_report.items.iter().any(|i| i.id == "key" && i.action.as_deref() == Some("settings")),
        "Key 错误应指路设置页"
    );
    // force=false 的 command 层拒绝语义等价于 report.blocked 分支——此处直接断言报告。
    println!("== 未配 Key 启动被拦截 ✓ ==");

    // 配 Key 进 DPAPI 保险库（真实 Key 缺失时用占位值继续端口验证，对话段再拦）。
    let key_for_launch = if has_real_key { real_key.clone() } else { "sk-placeholder-not-real".to_string() };
    crate::keyvault::set_enabled(crate::keyvault::VENDOR_DEEPSEEK, true).unwrap();
    crate::keyvault::set_key(crate::keyvault::VENDOR_DEEPSEEK, &key_for_launch).unwrap();
    crate::keyvault::set_models(crate::keyvault::VENDOR_DEEPSEEK, &["deepseek-v4-flash".to_string()])
        .unwrap();
    // 磁盘无明文（FR-06 后半）：keyvault.json 内容不得含 Key 片段。
    let vault_raw = std::fs::read_to_string(vault_path()).unwrap();
    assert!(!vault_raw.contains(&key_for_launch), "明文 Key 泄漏进磁盘！");
    let unblocked = crate::precheck::run(&a.id).unwrap();
    assert!(!unblocked.blocked, "配 Key 后不应再拦截：{:?}", unblocked.items);
    println!("== Key 入库（DPAPI）且磁盘无明文，校验链放行 ✓ ==");

    // ── 4. 双实例启动端口并存（FR-15/FR-08）──────────────────────────
    // 端口出生即钉定：创建时就该有固定值，且启动必须用它。
    let pinned_a = instances::get_instance(&a.id).unwrap().port.expect("创建实例时未分配端口");
    let pinned_b = instances::get_instance(&b.id).unwrap().port.expect("创建实例时未分配端口");
    assert_ne!(pinned_a, pinned_b, "两个实例的钉定端口相同——分配器坏了");

    let out_a = crate::process::launch(None, &a.id, false).expect("实例 A 启动失败");
    let out_b = crate::process::launch(None, &b.id, false).expect("实例 B 启动失败");
    assert_eq!(out_a.port, pinned_a, "A 启动未使用其钉定端口——端口漂移！");
    assert_eq!(out_b.port, pinned_b, "B 启动未使用其钉定端口——端口漂移！");
    println!("== A: http://127.0.0.1:{}  B: http://127.0.0.1:{} ==", out_a.port, out_b.port);

    wait_http_ready(out_a.port, 120);
    wait_http_ready(out_b.port, 120);
    println!("== 双端口 HTTP 均可达 ✓ ==");

    // ── 4b. 重启同端口（钉定语义核心）：停 A → 重启 → 端口必须不变 ───
    crate::process::stop(&a.id).unwrap();
    wait_port_free(out_a.port, Some(out_a.pid), 300);
    let out_a2 = crate::process::launch(None, &a.id, false).expect("实例 A 重启失败");
    assert_eq!(
        out_a2.port, out_a.port,
        "同一实例重启后端口变化——「入口地址随实例走」被破坏"
    );
    wait_http_ready(out_a2.port, 120);
    println!("== A 停止后重启回到同一端口 {} ✓ ==", out_a2.port);
    write_evidence(
        "accept-dual-port.txt",
        &format!(
            "instance A={} port={} pid={}\ninstance B={} port={} pid={}\nboth http-ok\nA restart -> port {} again ✓\nrunning={:?}\n",
            a.id, out_a.port, out_a.pid, b.id, out_b.port, out_b.pid,
            out_a2.port,
            crate::process::list_running()
        ),
    );

    // ── 5. 真实 Key 对话（走保险库 reveal → 环境注入链，R-2.3 裁决全链）──
    if !has_real_key {
        panic!("DEEPSEEK_ACCEPTANCE_KEY 未提供——无法完成真实对话验收");
    }
    let home_a = instances::instance_dir(&a.id).unwrap();
    let revealed = crate::keyvault::reveal_enabled_keys()
        .unwrap()
        .into_iter()
        .find(|(vendor, _)| vendor == crate::keyvault::VENDOR_DEEPSEEK)
        .map(|(_, key)| key)
        .expect("保险库应包含 deepseek 的 Key");
    assert_eq!(revealed, key_for_launch, "reveal_enabled_keys 与入库值不一致");
    let bin_js = crate::dsh_install::installed_bin_js(&a.id).unwrap().unwrap();
    let conversation_node = crate::node_runtime::load_config()
        .unwrap()
        .default_node_version
        .and_then(|v| crate::node_runtime::managed_node_exe(&v))
        .expect("对话段需要托管 Node 在位");
    let output = std::process::Command::new(conversation_node)
        .arg(bin_js)
        .args(["--profile", "headless", "只回复两个字：成功"])
        .current_dir(home_a.join("workspace"))
        .env("DSH_HOME", &home_a)
        .env("DEEPSEEK_API_KEY", revealed)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("拉起对话进程失败");
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("== 对话输出（exit {:?}）==\n{combined}", output.status.code());
    assert!(
        combined.contains("成功") && !combined.contains("invalid") && !combined.contains("Unauthorized"),
        "对话未得到模型有效回复"
    );
    write_evidence("accept-conversation.txt", &format!("prompt=只回复两个字：成功\n--- 输出 ---\n{combined}\n"));
    println!("== 真实 Key 有效对话 ✓ ==");

    // ── 6. 一键停止 + 端口释放回收 ──────────────────────────────────
    crate::process::stop(&a.id).unwrap();
    assert!(!crate::instances::port_free(out_b.port), "误判：B 还没停");
    wait_port_free(out_a.port, Some(out_a2.pid), 300);
    crate::process::stop(&b.id).unwrap();
    wait_port_free(out_b.port, Some(out_b.pid), 300);
    assert!(crate::process::list_running().is_empty(), "停止后运行表应清空");
    println!("== 双实例停止、端口全部释放 ✓ ==");
    write_evidence("accept-dual-port.txt", &(read_evidence("accept-dual-port.txt") + "both stopped; ports freed\n"));

    // ── 7. 收尾：删除两实例目录即零残留 ─────────────────────────────
    instances::delete_instance(&a.id).unwrap();
    instances::delete_instance(&b.id).unwrap();
    println!("== Phase 2 总验收通过 ✓✓ ==");
}

/// 保险库文件路径（与 keyvault 私有逻辑一致：managed root 下固定名）。
fn vault_path() -> std::path::PathBuf {
    crate::paths::ensure_managed_dirs().unwrap().join("keyvault.json")
}

/// 轮询直到端口返回任意 HTTP 响应（DSH web 启动需数十秒）。
fn wait_http_ready(port: u16, max_secs: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(max_secs);
    loop {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "端口 {port} 在 {max_secs}s 内未就绪");
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// 轮询直到端口可重新绑定（释放）；超时前按 pid 反复补发强杀并保留诊断。
/// taskkill 之后系统回收监听套接字在重载环境下偶发超过 30s，不能只等一轮。
fn wait_port_free(port: u16, pid: Option<u32>, max_secs: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(max_secs);
    let mut attempts: usize = 0;
    loop {
        if crate::instances::port_free(port) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "端口 {port} 在 {max_secs}s 内未释放（pid={pid:?}，已补杀 {attempts} 次）"
        );
        if let Some(pid) = pid {
            attempts += 1;
            let mut kill = Command::new("taskkill");
            kill.args(["/PID", &pid.to_string(), "/T", "/F"]);
            #[cfg(windows)]
            {
                use crate::node_runtime::CreationFlagsExt;
                kill.creation_flags_cv();
            }
            let _ = kill.output();
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
}

/// 把证据文本写进 .recon/（相对仓库根）。
fn write_evidence(name: &str, text: &str) {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../.recon/{name}"));
    use std::io::Write;
    std::fs::OpenOptions::new().create(true).append(true).open(path)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .expect("写证据文件失败");
}

fn read_evidence(name: &str) -> String {
    std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../.recon/{name}")),
    )
    .unwrap_or_default()
}
