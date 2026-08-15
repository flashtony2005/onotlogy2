//! Compensation —— P1 Failure Compensation 执行器。
//!
//! Fiber 失败 → 自动对已提交的可补偿 Effect 逆序补偿（C⁻¹ → B⁻¹ → A⁻¹）；
//! 补偿失败 → Fiber 升级 `Escalated` + 落事实（Relation: Effect --compensation_failed--> true），
//! 等待人工介入。补偿本身可审计（走 Kernel 的 events 事实层）。

use serde_json::Value;

use super::effect::EffectId;
use super::fiber::FiberState;
use super::relation::Relation;
use super::{Kernel, KernelError};

/// 补偿动作执行器（Phase 1：注入式；Phase 2+ 由 Service/Capability 层驱动）。
/// Err 表示补偿动作失败 → 升级人工。
pub trait Compensator: Send + Sync {
    fn compensate(&self, action: &str, payload: &Value) -> Result<(), String>;
}

/// 默认补偿器：声明式动作无实际副作用（测试注入确定性实现）。
pub struct NullCompensator;

impl Compensator for NullCompensator {
    fn compensate(&self, _action: &str, _payload: &Value) -> Result<(), String> {
        Ok(())
    }
}

/// 补偿引擎：持有 kernel 的可变借用，执行一次完整补偿序列。
pub struct CompensationEngine<'k> {
    kernel: &'k mut Kernel,
}

impl<'k> CompensationEngine<'k> {
    pub fn new(kernel: &'k mut Kernel) -> Self {
        Self { kernel }
    }

    /// 对 fiber 执行逆序补偿（P1）。
    ///
    /// 前置：fiber 处于 `Failed`（或 `Compensating`，幂等重入）。
    /// 成功路径：fiber → Compensating → 逐个逆序补偿 → Compensated。
    /// 失败路径：任一补偿失败 → fiber → Escalated + 事实 Relation，返回 Err（升级人工）。
    pub fn compensate(&mut self, fiber: super::FiberId) -> Result<(), KernelError> {
        // 前置状态：Failed 或 Compensating（重入幂等）
        let st = self
            .kernel
            .fiber(fiber)
            .map(|f| f.state)
            .ok_or(KernelError::FiberNotFound(fiber))?;
        if st != FiberState::Failed && st != FiberState::Compensating {
            return Err(KernelError::InvalidTransition(
                st.as_str().to_string(),
                "Compensating".into(),
            ));
        }
        self.kernel.advance(fiber, FiberState::Compensating)?;
        // Phase 8 · G4：补偿开始审计（可审计，而非"程序员觉得应该回滚"）
        self.kernel.record_event(
            Some(fiber),
            "compensation.started",
            serde_json::json!({ "fiber": format!("{fiber}") }),
        );

        // 逆序取已提交 Effect（含不可补偿项——Irreversible 跳过仅标记）
        let ledger: Vec<EffectId> = {
            let f = self
                .kernel
                .fiber(fiber)
                .ok_or(KernelError::FiberNotFound(fiber))?;
            f.ledger.reverse_order().copied().collect()
        };

        for id in ledger {
            let Some(effect) = self.kernel.effect(id).cloned() else {
                continue;
            };
            if !effect.is_compensatable() {
                continue; // Irreversible/Idempotent：不补偿，仅标记
            }
            let Some(comp) = effect.inverse.clone() else {
                continue; // 无反向定义：不可补偿项，跳过（Phase 2 可改为强制校验）
            };
            match self
                .kernel
                .compensator()
                .compensate(&comp.action, &comp.payload)
            {
                Ok(()) => {
                    self.kernel.mark_effect_compensated(id);
                    // Phase 8 · G4：单步补偿完成
                    self.kernel.record_event(
                        Some(fiber),
                        "compensation.completed",
                        serde_json::json!({ "effect": format!("{id}"), "action": comp.action }),
                    );
                    // 事实：Effect --compensated--> true
                    let _ = self.kernel.relate(Relation::new(
                        self.kernel
                            .fiber(fiber)
                            .map(|f| f.context)
                            .unwrap_or(super::ContextId(0)),
                        format!("{id}"),
                        "compensated",
                        "true",
                        self.kernel.now(),
                    ));
                }
                Err(reason) => {
                    self.kernel.mark_effect_compensation_failed(id);
                    // Phase 8 · G4：补偿失败不吞异常——双事件（补偿失败 + 人工介入）
                    self.kernel.record_event(
                        Some(fiber),
                        "compensation.failed",
                        serde_json::json!({ "effect": format!("{id}"), "action": comp.action, "reason": reason }),
                    );
                    // 升级人工：Fiber → Escalated + 事实 Relation
                    self.kernel.advance(fiber, FiberState::Escalated)?;
                    self.kernel.record_event(
                        Some(fiber),
                        "manual_intervention_required",
                        serde_json::json!({ "fiber": format!("{fiber}"), "reason": reason }),
                    );
                    let ctx = self
                        .kernel
                        .fiber(fiber)
                        .map(|f| f.context)
                        .unwrap_or(super::ContextId(0));
                    let _ = self.kernel.relate(Relation::new(
                        ctx,
                        format!("{id}"),
                        "compensation_failed",
                        "true",
                        self.kernel.now(),
                    ));
                    return Err(KernelError::Closed(format!(
                        "补偿失败（升级人工）: {} — {reason}",
                        comp.action
                    )));
                }
            }
        }

        self.kernel.advance(fiber, FiberState::Compensated)
    }
}

/// 供测试使用的确定性补偿器：记录调用顺序，可注入失败点。
pub struct RecordingCompensator {
    /// (action, payload) 调用记录（Arc 共享，测试断言顺序）。
    pub calls: std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>,
    /// 遇到该 action 时返回 Err（注入补偿失败）。
    pub fail_on: Option<String>,
}

impl RecordingCompensator {
    pub fn new() -> Self {
        Self {
            calls: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            fail_on: None,
        }
    }
}

impl Default for RecordingCompensator {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordingCompensator {
    pub fn with_failure(mut self, action: &str) -> Self {
        self.fail_on = Some(action.to_string());
        self
    }
}

impl Compensator for RecordingCompensator {
    fn compensate(&self, action: &str, payload: &Value) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((action.to_string(), payload.clone()));
        if self.fail_on.as_deref() == Some(action) {
            return Err(format!("补偿动作 {action} 注入失败"));
        }
        Ok(())
    }
}
