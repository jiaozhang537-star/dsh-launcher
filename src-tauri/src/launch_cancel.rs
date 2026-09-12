//! 「取消本次启动」令牌（需求方 2026-09-10 R3）。
//!
//! `instance_launch` 是一条跨多次 IO 的长链（校验 → 对账 → 停旧进程 → 同步配置 →
//! 拉起进程），取消必须能在**链上任何一个阶段**落地：链自己每过一个检查点就问一次
//! 「这次启动被取消了吗」，是就地收尾（不再往下走）；已经拉起的进程由取消命令自己
//! 停掉（进程可能比检查点先到位，所以两边都要动手，见 `instance_launch_cancel`）。
//!
//! 令牌按实例 id 记，且**每次启动开始时清空**——否则上一次的取消会误伤这一次。

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

fn cancelled() -> &'static Mutex<BTreeSet<String>> {
    static CANCELLED: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    CANCELLED.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// 标记该实例本次启动已取消；返回**是否为本次新登记**（重复请求返回 false，幂等）。
pub fn request(instance_id: &str) -> bool {
    cancelled()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(instance_id.to_string())
}

/// 清掉取消标记（每次启动开始时调用）。
pub fn clear(instance_id: &str) {
    cancelled()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(instance_id);
}

/// 本次启动是否已被取消。
pub fn is_cancelled(instance_id: &str) -> bool {
    cancelled()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains(instance_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 独立实例 id，避免与其他用例共享全局令牌表互相干扰。
    fn id(tag: &str) -> String {
        format!("i-cancel-{tag}")
    }

    #[test]
    fn unrequested_instance_is_not_cancelled() {
        let i = id("fresh");
        clear(&i);
        assert!(!is_cancelled(&i));
    }

    #[test]
    fn request_then_is_cancelled_is_true() {
        let i = id("basic");
        clear(&i);
        assert!(request(&i));
        assert!(is_cancelled(&i));
    }

    /// 重复点取消不能有任何副作用差异（返回 false 只表示「早就登记过了」）。
    #[test]
    fn repeated_request_is_idempotent() {
        let i = id("repeat");
        clear(&i);
        assert!(request(&i));
        assert!(!request(&i));
        assert!(is_cancelled(&i));
    }

    /// 令牌按实例隔离：取消 A 不影响 B（下一次启动 B 不该被误伤）。
    #[test]
    fn token_is_per_instance_and_clear_resets() {
        let (a, b) = (id("a"), id("b"));
        clear(&a);
        clear(&b);
        request(&a);
        assert!(is_cancelled(&a));
        assert!(!is_cancelled(&b));
        clear(&a);
        assert!(!is_cancelled(&a));
    }
}
