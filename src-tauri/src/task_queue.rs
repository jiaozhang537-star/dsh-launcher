//! 下载任务控制层（FR-09 W1）：每任务独立的「暂停 / 继续 / 取消」控制柄 +
//! 并发额度闸。
//!
//! 分工（W1 定稿）：任务展示态（状态机 / 进度 / 速度）归前端任务 store
//! 单一事实源；后端只持有「控制柄注册表」与「许可计数」，不重复维护任务
//! 快照——两侧经任务键（与前端 store 的 key 完全一致）对齐：
//! - Node 运行时下载：`node-<version>`
//! - 源码归档下载：`source-<scope>-<version>`（scope = 实例 id 或 `global`）
//! - 实例安装：`install-<instanceId>`
//! - 插件安装：`plugin-<spec>`

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// 取消错误的统一前缀：链路各层以 `starts_with` 识别，前端据此把任务
/// 标成「已取消」而非「失败」（与既有字符串错误风格一致，参照
/// source_archive 对 `status code 404` 的处理）。
pub const CANCELLED_PREFIX: &str = "任务已取消";

/// 构造带已接收字节数的取消错误（有断点信息的场景传 Some）。
pub fn cancelled_error(received: Option<u64>) -> String {
    match received {
        Some(n) => format!("{CANCELLED_PREFIX}（已接收 {}）", crate::node_runtime::fmt_size(n)),
        None => CANCELLED_PREFIX.to_string(),
    }
}

/// 单任务控制柄：取消立即生效；暂停在下一个检查点（字节流 ≤ 一个读块，
/// 子进程链 ≤ 一次输出 tick）生效，继续后从同一连接（若服务端未掐断）或
/// Range 断点（`.part` 仍在时自动重连续传）恢复。
pub struct TaskControl {
    cancel: AtomicBool,
    paused: AtomicBool,
    /// Condvar 的配对锁（不守卫数据，仅提供等待队列）。
    gate: Mutex<bool>,
    cond: Condvar,
}

impl TaskControl {
    fn new() -> Self {
        Self {
            cancel: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            gate: Mutex::new(true),
            cond: Condvar::new(),
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.cond.notify_all();
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.cond.notify_all();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// 检查点：已取消 → Err（[`CANCELLED_PREFIX`] 前缀）；暂停中 → 阻塞等待
    /// 直到继续 / 取消。字节流与子进程链在每个读块 / 输出 tick 调用一次。
    pub fn checkpoint(&self) -> Result<(), String> {
        loop {
            if self.is_cancelled() {
                return Err(cancelled_error(None));
            }
            if !self.paused.load(Ordering::SeqCst) {
                return Ok(());
            }
            let guard = self.gate.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // 带超时轮询：notify 全部覆盖 resume/cancel，超时仅作丢唤醒兜底。
            let _ = self.cond.wait_timeout(guard, Duration::from_millis(200));
        }
    }
}

/// 控制柄注册表（进程内；任务结束即移除，之后 pause/cancel 返回「不存在」）。
static CONTROLS: Mutex<Option<BTreeMap<String, Arc<TaskControl>>>> = Mutex::new(None);

fn with_controls<R>(f: impl FnOnce(&mut BTreeMap<String, Arc<TaskControl>>) -> R) -> R {
    let mut guard = CONTROLS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.get_or_insert_with(BTreeMap::new))
}

/// 注册任务控制柄（同键覆盖：前端对同键有防重入，此处兜底以最新为准）。
pub fn register(key: &str) -> Arc<TaskControl> {
    let control = Arc::new(TaskControl::new());
    with_controls(|m| m.insert(key.to_string(), control.clone()));
    control
}

/// 查控制柄（pause / resume / cancel 命令用）；任务不存在或已结束 → None。
pub fn lookup(key: &str) -> Option<Arc<TaskControl>> {
    with_controls(|m| m.get(key).cloned())
}

/// 任务结束（成功 / 失败 / 取消）时移除控制柄。
pub fn finish(key: &str) {
    with_controls(|m| m.remove(key));
}

/// 在用中的并发任务数。
static ACTIVE: AtomicU32 = AtomicU32::new(0);

/// 并发上限（FR-11 的「并发数」设置消费此值；W1 先落读取与默认，UI 留 W3）。
/// 默认 3，收敛 1~10，读取失败按默认。
pub fn max_downloads() -> u32 {
    crate::node_runtime::load_config()
        .ok()
        .and_then(|c| c.max_downloads)
        .unwrap_or(3)
        .clamp(1, 10)
}

/// 并发许可：持有即占一个额度，Drop 归还。等待额度期间逐次过检查点——
/// 排队中的任务被暂停（等待额度前先挂起）或取消（立即失败）都成立。
#[derive(Debug)]
pub struct Permit {
    _private: (),
}

impl Drop for Permit {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn acquire(control: &TaskControl) -> Result<Permit, String> {
    loop {
        control.checkpoint()?;
        let current = ACTIVE.load(Ordering::SeqCst);
        if current >= max_downloads() {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        }
        match ACTIVE.compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return Ok(Permit { _private: () }),
            Err(_) => continue, // 并发竞争：别的线程先拿到了，重试
        }
    }
}

/// 任务键控的断点续传临时文件：`<temp>/dsh-task-<key>.part`。键中非法
/// 文件名字符折叠为 `_`（键本身来自启动器内部约定，非用户输入）。
pub fn part_path(key: &str) -> PathBuf {
    let safe: String = key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .collect();
    std::env::temp_dir().join(format!("dsh-task-{safe}.part"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// 取消：置位后检查点报 CANCELLED_PREFIX 错误。
    #[test]
    fn checkpoint_fails_with_cancelled_prefix_after_cancel() {
        let control = TaskControl::new();
        assert!(control.checkpoint().is_ok());
        control.cancel();
        let err = control.checkpoint().unwrap_err();
        assert!(err.starts_with(CANCELLED_PREFIX), "实际：{err}");
    }

    /// 暂停：检查点阻塞直到 resume；恢复后继续 Ok。
    #[test]
    fn pause_blocks_checkpoint_until_resume() {
        let control = Arc::new(TaskControl::new());
        control.pause();
        let waiter = {
            let control = control.clone();
            std::thread::spawn(move || control.checkpoint())
        };
        // 200ms 轮询兜底下，未 resume 时必须仍阻塞（线程尚未结束）。
        std::thread::sleep(Duration::from_millis(400));
        assert!(!waiter.is_finished(), "暂停中的检查点不应自行返回");
        control.resume();
        assert!(waiter.join().unwrap().is_ok());
    }

    /// 暂停中取消：阻塞的检查点立即以取消错误返回（不要求先 resume）。
    #[test]
    fn cancel_wakes_paused_checkpoint() {
        let control = Arc::new(TaskControl::new());
        control.pause();
        let waiter = {
            let control = control.clone();
            std::thread::spawn(move || control.checkpoint())
        };
        std::thread::sleep(Duration::from_millis(200));
        control.cancel();
        let err = waiter.join().unwrap().unwrap_err();
        assert!(err.starts_with(CANCELLED_PREFIX), "实际：{err}");
    }

    /// 注册表：register → lookup 同柄；finish 后 lookup 为 None；
    /// 同键覆盖后旧柄失效（lookup 返回新柄）。
    #[test]
    fn registry_register_lookup_finish_and_overwrite() {
        let a = register("test-reg-a");
        assert!(Arc::ptr_eq(&a, &lookup("test-reg-a").unwrap()));
        finish("test-reg-a");
        assert!(lookup("test-reg-a").is_none());

        let b = register("test-reg-b");
        let _b2 = register("test-reg-b");
        assert!(!Arc::ptr_eq(&b, &lookup("test-reg-b").unwrap()));
        finish("test-reg-b");
    }

    /// 许可闸：acquire 占额、Drop 归还；排队等待期间取消立即失败。
    #[test]
    fn permit_acquire_drop_and_cancel_while_waiting() {
        // 归零起点（其他测试可能留有残留许可——本测试独占语义要求空表）。
        // 直接构造满额：手动灌满 ACTIVE。
        let max = max_downloads();
        for _ in 0..max {
            ACTIVE.fetch_add(1, Ordering::SeqCst);
        }
        let control = Arc::new(TaskControl::new());
        let waiter = {
            let control = control.clone();
            std::thread::spawn(move || acquire(&control))
        };
        std::thread::sleep(Duration::from_millis(400));
        assert!(!waiter.is_finished(), "满额时 acquire 应仍在等待");
        control.cancel();
        let err = waiter.join().unwrap().unwrap_err();
        assert!(err.starts_with(CANCELLED_PREFIX), "实际：{err}");

        // 归还灌满的额度（模拟其他任务结束），再验证 acquire/Drop 计数回归。
        for _ in 0..max {
            ACTIVE.fetch_sub(1, Ordering::SeqCst);
        }
        let fresh = TaskControl::new();
        let permit = acquire(&fresh).unwrap();
        drop(permit);
        assert_eq!(ACTIVE.load(Ordering::SeqCst), 0);
    }

    /// part 路径：任务键折叠非法字符，落临时目录。
    #[test]
    fn part_path_sanitizes_key() {
        let p = part_path("node-22.20.0");
        assert!(p.file_name().unwrap().to_string_lossy().contains("node-22.20.0"));
        let q = part_path("source/global-0.1.1-rc.2");
        let name = q.file_name().unwrap().to_string_lossy().into_owned();
        assert!(!name.contains('/'), "路径分隔符必须被折叠：{name}");
        assert!(name.ends_with(".part"));
        assert!(q.parent().unwrap() == std::env::temp_dir());
    }
}
