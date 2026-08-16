use std::sync::Mutex;
use std::sync::MutexGuard;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// HTTP 审批挂起的最长等待时间；超时按拒绝处理（不授予权限）。
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

/// 获取互斥锁：poison 时恢复内层数据（不 panic），避免审批/缓存/runs 映射路径因 poison 整进程崩溃。
/// server.rs 与 planner.rs 共用同一实现（Mutex 中毒统一处理）。
pub(crate) fn lock_mutex<'a, T>(m: &'a Mutex<T>) -> MutexGuard<'a, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// object_refs 非空约束（WB2006）的兜底：无具体对象时用 "world" 哨兵表示"针对整个世界"。
pub(super) fn non_empty(refs: Vec<String>) -> Vec<String> {
    if refs.is_empty() {
        vec!["world".to_string()]
    } else {
        refs
    }
}

/// 当前 unix 微秒（业务时间戳）。
pub(super) fn now_micros() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
        .to_string()
}

/// 预生成运行 ID（server 在触发运行前占坑状态映射）。
/// M1b G6：实现已迁至 `session::run`（planner 退役后仍存活）。
pub fn generate_run_id() -> String {
    crate::session::run::generate_run_id()
}
