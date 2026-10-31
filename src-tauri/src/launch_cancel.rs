//! 「取消本次启动」令牌（需求方 2026-09-10 R3）。
//!
//! `instance_launch` 是一条跨多次 IO 的长链（校验 → 对账 → 停旧进程 → 同步配置 →
//! 拉起进程），取消必须能在**链上任何一个阶段**落地：链自己每过一个检查点就问一次
//! 「这次启动被取消了吗」，是就地收尾；已经拉起的进程由取消命令自己停掉。
//!
//! 令牌**按代记**（对抗审查 R1 2026-10-05 P1-03）：每次启动开始时代际自增，
//! 取消命令标记的是「当前活跃代」，启动链的检查点只问自己的代——于是「取消后
//! 立刻再启动」时，新启动不再清掉旧启动还没消费的取消标记（旧实现是单槽
//! set + 启动时清空，两链并发时旧链会复活），旧代标记也无需清理：代严格
//! 递增，旧标记永不被新链命中。

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

/// 各实例的当前代（每 begin 一次 +1；未启动过的实例视为第 0 代）。
fn generations() -> &'static Mutex<HashMap<String, u64>> {
    static GENS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    GENS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 已登记的取消标记（实例 id, 代）。
fn cancelled() -> &'static Mutex<HashSet<(String, u64)>> {
    static CANCELLED: OnceLock<Mutex<HashSet<(String, u64)>>> = OnceLock::new();
    CANCELLED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn current_gen(instance_id: &str) -> u64 {
    generations()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(instance_id)
        .copied()
        .unwrap_or(0)
}

/// 当前活跃代快照。crate 内复用：就绪监视线程 spawn 时取一份，此后发现
/// 「当前代 ≠ 自己的代」即静默退役（新启动已开始，旧监视不得再发事件）。
pub fn current_gen_of(instance_id: &str) -> u64 {
    current_gen(instance_id)
}

/// 新一次启动开始：代际自增并返回本次启动的代。
///
/// 刻意**不**清除旧代标记：若上一次启动链还活着（取消后立刻再启动），
/// 它的检查点仍按旧代查询、必须继续命中取消。
pub fn begin(instance_id: &str) -> u64 {
    let mut gens = generations().lock().unwrap_or_else(|p| p.into_inner());
    let gen = gens.entry(instance_id.to_string()).or_insert(0);
    *gen += 1;
    *gen
}

/// 标记该实例**当前代**的启动已取消（取消命令调用；幂等）。
pub fn request(instance_id: &str) {
    cancelled()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert((instance_id.to_string(), current_gen(instance_id)));
}

/// 指定代的启动是否已被取消（启动链检查点用，带自己 begin 拿到的代）。
pub fn is_cancelled(instance_id: &str, gen: u64) -> bool {
    cancelled()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains(&(instance_id.to_string(), gen))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 独立实例 id，避免与其他用例共享全局令牌表互相干扰。
    fn id(tag: &str) -> String {
        format!("i-cancel-{tag}")
    }

    #[test]
    fn fresh_instance_is_not_cancelled() {
        let i = id("fresh");
        assert!(!is_cancelled(&i, 1));
        assert!(!is_cancelled(&i, current_gen_of(&i)));
    }

    #[test]
    fn request_marks_current_generation() {
        let i = id("basic");
        let gen = begin(&i);
        assert!(gen >= 1);
        request(&i);
        assert!(is_cancelled(&i, gen));
        assert!(is_cancelled(&i, current_gen_of(&i)));
    }

    /// 重复点取消不能有任何副作用差异。
    #[test]
    fn repeated_request_is_idempotent() {
        let i = id("repeat");
        let gen = begin(&i);
        request(&i);
        request(&i);
        assert!(is_cancelled(&i, gen));
    }

    /// 代际隔离（P1-03 的病灶回归）：旧链的取消标记不得误伤新链，
    /// 新链的取消也不影响旧代查询。
    #[test]
    fn generations_isolate_concurrent_launches() {
        let i = id("gen");
        let gen1 = begin(&i);
        request(&i); // L1 被取消
        let gen2 = begin(&i); // L2 立刻开始——不清旧标记
        assert_ne!(gen1, gen2);
        assert!(is_cancelled(&i, gen1), "旧链检查点必须仍命中取消");
        assert!(!is_cancelled(&i, gen2), "新链不得被旧标记误伤");
        assert!(!is_cancelled(&i, current_gen_of(&i)), "当前代未被取消");
        request(&i); // 取消 L2
        assert!(is_cancelled(&i, current_gen_of(&i)));
        assert!(is_cancelled(&i, gen1), "旧代标记仍在（无需清理，永不误命中）");
    }

    /// 代严格递增：同一实例反复 begin 不会回到旧代。
    #[test]
    fn begin_is_strictly_increasing() {
        let i = id("mono");
        let a = begin(&i);
        let b = begin(&i);
        let c = begin(&i);
        assert!(a < b && b < c);
    }

    /// 令牌按实例隔离：取消 A 不影响 B。
    #[test]
    fn token_is_per_instance() {
        let (a, b) = (id("a"), id("b"));
        let gen_a = begin(&a);
        let gen_b = begin(&b);
        request(&a);
        assert!(is_cancelled(&a, gen_a));
        assert!(!is_cancelled(&b, gen_b));
    }
}
