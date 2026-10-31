//! 无头 CLI（P7.5 / FR-26）：`dsh-launcher.exe` 带子命令时的执行面。
//!
//! 协议：**stdout 纯 JSON**（机器可读，`help` 除外）+ **stderr 中文进度/错误**
//! （人类可读）+ exit code **0 成功 / 1 业务失败 / 2 用法错误**。与 GUI 共用
//! 同一套启动管线（`launch_flow::launch_pipeline`）与 `running.json` 状态，
//! 两个入口互认、互斥（文件级启动标记）。
//!
//! AI 实验的典型循环：
//! `launch <id>`（默认等到网页就绪，返回带令牌 webUrl——裸端口 401，令牌 URL
//! 是唯一正确入口）→ 改实验变量 → `restart <id>`（端口不漂移、新令牌）→
//! `logs <id>` 看输出 → `stop <id>`。

use serde_json::{json, Value};

use crate::{instances, launch_flow, process};

/// CLI 入口：返回进程退出码。`argv` 不含程序名。
pub fn main(argv: Vec<String>) -> i32 {
    attach_console_stdio();
    let mut args = argv.into_iter();
    let Some(cmd) = args.next() else {
        return usage_fail("缺少子命令");
    };
    let rest: Vec<String> = args.collect();
    match cmd.as_str() {
        "ls" | "list" => finish(cmd_ls()),
        // restart 与 launch 同一条链：「在跑就先停、按钉定端口拉起」内建于编排。
        "launch" | "restart" => finish(cmd_launch(&rest)),
        "stop" => finish(cmd_stop(&rest)),
        "status" => finish(cmd_status(&rest)),
        "logs" => finish(cmd_logs(&rest)),
        "help" | "--help" | "-h" => {
            print!("{}", usage_text());
            0
        }
        other => usage_fail(&format!("未知子命令「{other}」")),
    }
}

/// 业务错误：code 1（业务失败）或 2（用法错误）；payload 进 stdout JSON 的
/// `detail` 字段（如被拦截的校验报告、超时时的进程信息）。
struct CliError {
    code: i32,
    message: String,
    payload: Option<Value>,
}

impl CliError {
    fn fail(message: impl Into<String>) -> Self {
        Self { code: 1, message: message.into(), payload: None }
    }
    fn with_payload(message: impl Into<String>, payload: Value) -> Self {
        Self { code: 1, message: message.into(), payload: Some(payload) }
    }
}

type CliOk = (Value, Option<String>);

fn finish(result: Result<CliOk, CliError>) -> i32 {
    match result {
        Ok((value, note)) => {
            if let Some(note) = note {
                eprintln!("✔ {note}");
            }
            print_json(&value);
            0
        }
        Err(err) => {
            eprintln!("✘ {}", err.message);
            let mut value = json!({ "error": err.message });
            if let Some(payload) = err.payload {
                value["detail"] = payload;
            }
            print_json(&value);
            err.code
        }
    }
}

fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".into())
    );
}

fn usage_fail(message: &str) -> i32 {
    eprintln!("用法错误：{message}\n\n{}", usage_text());
    print_json(&json!({ "error": format!("用法错误：{message}"), "detail": null }));
    2
}

fn usage_text() -> String {
    "用法：dsh-launcher.exe <子命令> [参数]\n\
     \x20 ls                       实例清单（含运行态 / 端口 / webUrl）\n\
     \x20 launch <实例> [--force] [--wait ready|detached] [--timeout 秒]\n\
     \x20                           启动（默认等到网页就绪并返回带令牌 webUrl；\n\
     \x20                           实例已在运行时 = 按原端口重启，端口不漂移）\n\
     \x20 restart <实例> [--force] [--wait ready|detached] [--timeout 秒]\n\
     \x20                           重启（与 launch 同一条链的显式写法）\n\
     \x20 stop <实例>              停止进程树（未在运行时幂等成功）\n\
     \x20 status [<实例>|--all]    运行状态（缺省 = 全部实例）\n\
     \x20 logs <实例> [--tail N]   查看命令行拉起实例的落盘控制台日志（默认尾 50 行）\n\
     \x20 help                     本说明\n\
     \n\
     \x20<实例> = 实例标识（ls 输出的 instanceId，即目录名）。\n"
        .to_string()
}

// ── ls / status ────────────────────────────────────────────────────────────

fn instance_entry(inst: &instances::Instance) -> Value {
    let state = process::process_state(&inst.id);
    let web_url = match &state {
        Ok(st) if st.running && st.web_url.is_none() => {
            // detached 拉起后 URL 行可能还没进 running.json——扫日志最后一个
            // 启动段补上（仅在进程仍在运行时，旧 token 无效不作数）。
            process::recover_web_url_from_log(&inst.id)
        }
        Ok(st) => st.web_url.clone(),
        Err(_) => None,
    };
    match state {
        Ok(st) => json!({
            "instanceId": inst.id,
            "name": inst.name.clone(),
            "profile": inst.profile.clone(),
            "dshVersion": inst.dsh_version.clone(),
            "running": st.running,
            "adopted": st.adopted,
            "pid": st.pid,
            "port": st.port,
            "restartNeeded": st.restart_needed,
            "webUrl": web_url,
            "lastLaunchedAt": inst.last_launched_at.clone(),
            "isolation": inst.isolation,
        }),
        Err(err) => json!({
            "instanceId": inst.id,
            "name": inst.name.clone(),
            "profile": inst.profile.clone(),
            "dshVersion": inst.dsh_version.clone(),
            "running": false,
            "adopted": false,
            "pid": null,
            "port": inst.port,
            "restartNeeded": null,
            "webUrl": null,
            "lastLaunchedAt": inst.last_launched_at.clone(),
            "isolation": inst.isolation,
            "stateError": err,
        }),
    }
}

fn list_entries() -> Result<Vec<Value>, CliError> {
    let mut list = instances::list_instances().map_err(CliError::fail)?;
    // 最近使用置顶（与主页同一心智）；从未启动的排最后、彼此按创建序。
    list.sort_by(|a, b| b.last_launched_at.cmp(&a.last_launched_at));
    Ok(list.iter().map(instance_entry).collect())
}

fn cmd_ls() -> Result<CliOk, CliError> {
    let entries = list_entries()?;
    let running = entries.iter().filter(|e| e["running"] == json!(true)).count();
    Ok((
        json!({ "instances": entries }),
        Some(format!("共 {} 个实例，{} 个运行中", entries.len(), running)),
    ))
}

fn cmd_status(rest: &[String]) -> Result<CliOk, CliError> {
    // --all 显式全查；缺省（无实例参数）也是全查；给了实例参数则单查。
    let explicit_all = rest.iter().any(|a| a == "--all");
    let target = positional(rest, 0, "status");
    if explicit_all || target.is_err() {
        let entries = list_entries()?;
        return Ok((json!({ "instances": entries }), None));
    }
    let id = target.expect("上面已排除 Err");
    let inst = instances::get_instance(&id)
        .map_err(|err| CliError::fail(format!("查不到实例「{id}」：{err}")))?;
    Ok((instance_entry(&inst), None))
}

// ── launch / restart ───────────────────────────────────────────────────────

struct LaunchOpts {
    id: String,
    wait_ready: bool,
    timeout_secs: u64,
    force: bool,
}

fn parse_launch_args(rest: &[String]) -> Result<LaunchOpts, String> {
    let mut id: Option<String> = None;
    let mut wait_ready = true;
    let mut timeout_secs: u64 = 180;
    let mut force = false;
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        match arg {
            "--force" => force = true,
            "--wait" => {
                i += 1;
                let mode = rest.get(i).ok_or("--wait 需要值：ready 或 detached")?;
                wait_ready = match mode.as_str() {
                    "ready" => true,
                    "detached" => false,
                    other => return Err(format!("--wait 不认得「{other}」：只能是 ready 或 detached")),
                };
            }
            "--timeout" => {
                i += 1;
                let raw = rest.get(i).ok_or("--timeout 需要秒数（如 --timeout 300）")?;
                timeout_secs = raw.parse().map_err(|_| format!("--timeout 不是数字：「{raw}」"))?;
            }
            other => {
                if other.starts_with("--") {
                    return Err(format!("未知参数「{other}」"));
                }
                if id.replace(other.to_string()).is_some() {
                    return Err("只能给一个实例参数".into());
                }
            }
        }
        i += 1;
    }
    Ok(LaunchOpts {
        id: id.ok_or("缺少实例参数（ls 可查 instanceId）")?,
        wait_ready,
        timeout_secs,
        force,
    })
}

fn cmd_launch(rest: &[String]) -> Result<CliOk, CliError> {
    let opts = parse_launch_args(rest).map_err(|msg| CliError { code: 2, message: msg, payload: None })?;
    let mut progress = |percent: u8, stage: &str| {
        eprintln!("→ [{percent}%] {stage}");
    };
    let result = launch_flow::launch_pipeline(None, &opts.id, opts.force, &mut progress);
    let result = match result {
        Ok(result) => result,
        // 校验被拦截：Err 是报告 JSON 串（与 GUI 契约一致）。
        Err(message) => {
            return match serde_json::from_str::<Value>(&message) {
                Ok(report) if report.get("blocked") == Some(&json!(true)) => {
                    Err(CliError::with_payload(
                        "启动前校验未通过（错误级条目拦截）——完整报告在 detail 字段；确认要放行时加 --force",
                        report,
                    ))
                }
                _ => Err(CliError::fail(message)),
            };
        }
    };
    let port = result.port;
    let pid = result.pid;
    let mut value = json!({
        "instanceId": result.instance_id,
        "port": port,
        "pid": pid,
        "report": serde_json::to_value(&result.report).unwrap_or(Value::Null),
        "reconcile": serde_json::to_value(&result.reconcile).unwrap_or(Value::Null),
    });
    if !opts.wait_ready {
        value["status"] = json!("launched");
        value["webUrl"] = json!(process::process_state(&opts.id).ok().and_then(|s| s.web_url));
        return Ok((value, Some("已拉起（detached：不等待网页就绪，稍后用 status 查）".into())));
    }
    let log = result.log.as_ref().ok_or_else(|| CliError::fail("内部错误：无日志落点"))?;
    let log_path = std::path::PathBuf::from(&log.path);
    eprintln!("→ [72%] 等待 DSH 装载插件层并开放网页…");
    match process::wait_web_ready_from_log(
        &opts.id,
        port,
        pid,
        &log_path,
        log.start_offset,
        std::time::Duration::from_secs(opts.timeout_secs),
    ) {
        process::WebReady::Url(url) => {
            value["status"] = json!("ready");
            value["webUrl"] = json!(url);
            value["urlSource"] = json!("token");
            Ok((value, Some("网页已就绪——webUrl 可直接在浏览器打开".into())))
        }
        process::WebReady::PortOnly => {
            value["status"] = json!("ready");
            value["webUrl"] = json!(format!("http://127.0.0.1:{port}/"));
            value["urlSource"] = json!("port-fallback");
            Ok((
                value,
                Some("端口已开放（该版本不打印带令牌的 URL 行，回落裸端口地址）".into()),
            ))
        }
        process::WebReady::Timeout => Err(CliError::with_payload(
            format!(
                "等待网页就绪超时（{} 秒）——进程仍在运行，可稍后用 status 查询、logs 看输出",
                opts.timeout_secs
            ),
            json!({ "instanceId": opts.id, "port": port, "pid": pid, "status": "timeout" }),
        )),
        process::WebReady::Exited => Err(CliError::with_payload(
            "实例进程在就绪前退出了——用 logs 命令查看输出定位原因".to_string(),
            json!({ "instanceId": opts.id, "port": port, "pid": pid, "status": "exited" }),
        )),
    }
}

// ── stop / logs ────────────────────────────────────────────────────────────

fn cmd_stop(rest: &[String]) -> Result<CliOk, CliError> {
    let id = positional(rest, 0, "stop").map_err(|_| CliError {
        code: 2,
        message: "stop 需要一个实例参数".into(),
        payload: None,
    })?;
    instances::get_instance(&id)
        .map_err(|err| CliError::fail(format!("查不到实例「{id}」：{err}")))?;
    let was_running = process::process_state(&id).map(|s| s.running).unwrap_or(false);
    process::stop(&id).map_err(CliError::fail)?;
    let note = if was_running {
        "已停止实例进程".to_string()
    } else {
        "实例本就未在运行（幂等成功）".to_string()
    };
    Ok((json!({ "instanceId": id, "wasRunning": was_running }), Some(note)))
}

fn cmd_logs(rest: &[String]) -> Result<CliOk, CliError> {
    let id = positional(rest, 0, "logs").map_err(|_| CliError {
        code: 2,
        message: "logs 需要一个实例参数".into(),
        payload: None,
    })?;
    let mut tail: usize = 50;
    let mut i = 1;
    while i < rest.len() {
        match rest[i].as_str() {
            "--tail" => {
                i += 1;
                let raw = rest.get(i).ok_or_else(|| CliError {
                    code: 2,
                    message: "--tail 需要行数（如 --tail 200）".into(),
                    payload: None,
                })?;
                tail = raw.parse().map_err(|_| CliError {
                    code: 2,
                    message: format!("--tail 不是数字：「{raw}」"),
                    payload: None,
                })?;
            }
            other => {
                return Err(CliError {
                    code: 2,
                    message: format!("未知参数「{other}」"),
                    payload: None,
                })
            }
        }
        i += 1;
    }
    instances::get_instance(&id)
        .map_err(|err| CliError::fail(format!("查不到实例「{id}」：{err}")))?;
    let path = process::console_log_path(&id).map_err(CliError::fail)?;
    if !path.exists() {
        return Ok((
            json!({
                "instanceId": id,
                "path": path.display().to_string(),
                "totalLines": 0,
                "lines": [],
                "note": "该实例未经命令行拉起（界面会话拉起不落盘）或尚无输出",
            }),
            None,
        ));
    }
    let text = crate::paths::read_text(&path).map_err(CliError::fail)?;
    let all: Vec<&str> = text.lines().collect();
    let total = all.len();
    let lines: Vec<&str> = if total > tail { all[total - tail..].to_vec() } else { all };
    Ok((
        json!({
            "instanceId": id,
            "path": path.display().to_string(),
            "totalLines": total,
            "lines": lines,
        }),
        None,
    ))
}

// ── 参数小件 ───────────────────────────────────────────────────────────────

fn positional(rest: &[String], index: usize, _cmd: &str) -> Result<String, ()> {
    rest.iter()
        .filter(|a| !a.starts_with("--"))
        .nth(index)
        .cloned()
        .ok_or(())
}

// ── Windows 控制台挂接 ─────────────────────────────────────────────────────
// release 构建是 windows 子系统（无控制台）：从终端里跑 CLI 时要把输出挂回
// 父进程控制台，否则 JSON 全部落空。重定向/管道（AI 场景的主流）下 std 句柄
// 本就有效，直接用。

#[cfg(windows)]
fn attach_console_stdio() {
    use std::os::windows::io::AsRawHandle as _;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::{
        AttachConsole, GetStdHandle, SetConsoleOutputCP, SetStdHandle, ATTACH_PARENT_PROCESS,
        STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    };

    fn handle_ok(handle: HANDLE) -> bool {
        let ptr = handle.0 as usize;
        ptr != 0 && ptr != usize::MAX
    }

    unsafe {
        let null_handle = HANDLE(std::ptr::null_mut());
        let out_ok = handle_ok(GetStdHandle(STD_OUTPUT_HANDLE).unwrap_or(null_handle));
        let err_ok = handle_ok(GetStdHandle(STD_ERROR_HANDLE).unwrap_or(null_handle));
        if out_ok && err_ok {
            return;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS).is_ok() {
            // CONOUT$ 即本控制台输出屏；句柄所有权移交 std（忘记释放，随进程收尾）。
            if let Ok(console) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
                let handle = HANDLE(console.as_raw_handle());
                std::mem::forget(console);
                let _ = SetStdHandle(STD_OUTPUT_HANDLE, handle);
                let _ = SetStdHandle(STD_ERROR_HANDLE, handle);
            }
            // 控制台切 UTF-8：中文文案不至于按默认 GBK 显示成乱码。
            let _ = SetConsoleOutputCP(65001);
        }
    }
}

#[cfg(not(windows))]
fn attach_console_stdio() {}

// ── 单测：纯解析与形状（不触托管根；真机行为归冒烟） ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|i| i.to_string()).collect()
    }

    #[test]
    fn launch_args_matrix() {
        let ok = parse_launch_args(&s(&["dsh-0.2.0"])).unwrap();
        assert_eq!(ok.id, "dsh-0.2.0");
        assert!(ok.wait_ready && !ok.force && ok.timeout_secs == 180);

        let ok = parse_launch_args(&s(&["a", "--force", "--wait", "detached", "--timeout", "5"])).unwrap();
        assert!((!ok.wait_ready) && ok.force && ok.timeout_secs == 5);

        assert!(parse_launch_args(&s(&[])).is_err(), "缺实例");
        assert!(parse_launch_args(&s(&["a", "b"])).is_err(), "双实例");
        assert!(parse_launch_args(&s(&["a", "--wat"])).is_err(), "未知旗标");
        assert!(parse_launch_args(&s(&["a", "--wait", "soon"])).is_err(), "wait 值非法");
        assert!(parse_launch_args(&s(&["a", "--timeout"])).is_err(), "timeout 缺值");
        assert!(parse_launch_args(&s(&["a", "--timeout", "x"])).is_err(), "timeout 非数字");
    }

    #[test]
    fn positional_flags_are_skipped() {
        let rest = s(&["--force", "dsh-1", "--wait"]);
        assert_eq!(positional(&rest, 0, "launch").unwrap(), "dsh-1");
        assert!(positional(&s(&["--all"]), 0, "status").is_err());
    }

    #[test]
    fn unknown_subcommand_is_usage_error() {
        // 不触托管根：用法错误在进入任何命令实现之前就返回。
        assert_eq!(main(s(&["bogus"])), 2);
        assert_eq!(main(s(&[])), 2);
    }
}
