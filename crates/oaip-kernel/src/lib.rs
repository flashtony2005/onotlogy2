//! Runtime Kernel V2.0 —— Spatiotemporal Ontology Runtime（L0 四 Primitive）。
//!
//! 设计定稿：docs/03_架构设计/runtime-kernel-v2.md
//!
//! 四个 Primitive（Kernel Algebra）：
//! - `Context`：时空作用域（我在哪里存在）
//! - `Fiber`：世界中的生命体（谁正在经历时间）
//! - `Effect`：可审计副作用 + 补偿（它改变了什么）
//! - `Relation`：动态图（谁和谁发生了什么关系）
//!
//! 四条 Architecture Principles：
//! - P1 Failure Compensation：Fiber 失败自动逆序补偿，补偿失败升级人工
//! - P2 Lifecycle Isolation：Context 销毁终止 Fiber 但不隐式补偿业务副作用
//! - P3 Temporal Minimalism：Kernel 只保证当前有效状态（as_of 属 Phase 4）
//! - P4 Cognitive Separation：LLM 推理是 poll 步骤，不入 fact 层（Phase 1+ 实现 poll）
//!
//! Phase 0：纯内存实现 + 状态机 + 单测，不接持久化、不接业务层。
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

use serde_json::{Value, json};
use std::collections::HashMap;

pub mod compensation;
pub mod context;
pub mod effect;
pub mod event_ref;
pub mod fiber;
pub mod persistence;
pub mod projection;
pub mod relation;
pub mod scheduler;
pub mod state;

pub use compensation::{CompensationEngine, Compensator, NullCompensator, RecordingCompensator};
pub use context::{Context, ContextId, ContextState, Scope};
pub use effect::{Compensation, Effect, EffectId, EffectLedger, EffectState, Reversibility};
pub use event_ref::EventRef;
pub use fiber::{Fiber, FiberId, FiberState, WaitPattern};
pub use relation::{Relation, RelationId};
pub use state::StateStore;

/// Kernel 错误：全部显式建模，绝不 panic。
#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("Context 不存在: {0}")]
    ContextNotFound(ContextId),
    #[error("Fiber 不存在: {0}")]
    FiberNotFound(FiberId),
    #[error("Effect 不存在: {0}")]
    EffectNotFound(EffectId),
    #[error("Context 已销毁: {0}")]
    ContextDisposed(ContextId),
    #[error("Context 不在有效时间窗内（valid_from/valid_until）: {0}")]
    ContextExpired(ContextId),
    #[error("非法状态转换 {0} → {1}")]
    InvalidTransition(String, String),
    #[error("Fiber 不在 RUNNING 状态，无法 {0}")]
    FiberNotRunning(String),
    #[error("fiber {0} 已在其 context {1} 之外运行")]
    FiberContextMismatch(FiberId, ContextId),
    #[error("Relation 不存在: {0}")]
    RelationNotFound(RelationId),
    #[error("无法生成新动作：{0}")]
    Closed(String),
}

/// 当前 unix 秒（Kernel 时钟，测试可注入）。
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 最小运行时内核（Phase 0：纯内存聚合）。
///
/// 只提供五个核心动作 + 显式恢复信号：
/// create_context / spawn / emit / relate / dispose(+dispose_context) / resume
pub struct Kernel {
    contexts: HashMap<ContextId, Context>,
    fibers: HashMap<FiberId, Fiber>,
    effects: HashMap<EffectId, Effect>,
    relations: HashMap<RelationId, Relation>,
    next_ctx: u64,
    next_fiber: u64,
    next_effect: u64,
    next_relation: u64,
    /// 可注入时钟（测试固定时间验证 P3 过期硬约束）。
    clock: fn() -> i64,
    /// P1 补偿动作执行器（默认 Null）。
    compensator: std::sync::Arc<dyn Compensator>,
    /// 事实层事件（emit/relate/compensate 的审计记录；Phase 1 落 events 表）。
    events: Vec<KernelEvent>,
    /// P5.6：审计事件同步回写 sink（record_event 产生时调用；None = 无落库）。
    audit_sink: Option<std::sync::Arc<dyn AuditSink>>,
    /// Phase 8：幂等执行去重（context + key → 已执行 Effect）。
    idempotency: std::collections::HashMap<(ContextId, String), EffectId>,
}

impl Default for Kernel {
    fn default() -> Self {
        Self {
            contexts: HashMap::new(),
            fibers: HashMap::new(),
            effects: HashMap::new(),
            relations: HashMap::new(),
            next_ctx: 0,
            next_fiber: 0,
            next_effect: 0,
            next_relation: 0,
            clock: now_secs,
            compensator: std::sync::Arc::new(NullCompensator),
            events: Vec::new(),
            idempotency: std::collections::HashMap::new(),
            audit_sink: None,
        }
    }
}

/// P5.6：审计事件同步回写 sink——Kernel 审计事件（approval.* /
/// evolution.gate_approved 等）**产生时**即同步落库，不依赖 run 终局桥接
/// （契约 projection-v1.md §6.5：Event Log 成为 Audit / Projection /
/// Evaluation / Replay 的单一扇出源）。实现须同步（block_on 语义）。
/// 落库失败不阻断执行：审计是旁路扇出，主路径仍是 Kernel 事件内存流。
pub trait AuditSink: Send + Sync {
    fn append(&self, ev: &KernelEvent) -> Result<(), String>;
}

impl Kernel {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注入固定时钟（测试用）。
    pub fn with_clock(clock: fn() -> i64) -> Self {
        let mut k = Self::new();
        k.clock = clock;
        k
    }

    /// 当前内核时钟（组合层/上层使用）。
    pub fn now(&self) -> i64 {
        (self.clock)()
    }

    /// 注入补偿执行器（P1）。
    pub fn with_compensator(mut self, c: std::sync::Arc<dyn Compensator>) -> Self {
        self.compensator = c;
        self
    }

    /// P5.6：注入审计 sink——record_event 产生时同步落库（None = 纯内存）。
    pub fn with_audit_sink(mut self, sink: std::sync::Arc<dyn AuditSink>) -> Self {
        self.audit_sink = Some(sink);
        self
    }

    pub fn compensator(&self) -> std::sync::Arc<dyn Compensator> {
        self.compensator.clone()
    }

    /// 事实层事件（审计）。
    pub fn events(&self) -> &[KernelEvent] {
        &self.events
    }

    fn record_event(&mut self, fiber: Option<FiberId>, ty: &str, payload: Value) {
        let ctx = fiber.and_then(|f| self.fibers.get(&f).map(|x| x.context));
        let ev = KernelEvent {
            context: ctx,
            fiber,
            ty: ty.to_string(),
            payload,
            timestamp: self.now(),
        };
        // P5.6：产生时即落库（sink 失败不阻断——审计旁路；事件流不受影响）
        if let Some(sink) = &self.audit_sink {
            let _ = sink.append(&ev);
        }
        self.events.push(ev);
    }

    /// 组合层：记录一次策略拒绝（业务拒绝，非运行失败；Effect 未提交）。
    pub fn record_policy_denied(&mut self, fiber: FiberId, capability: &str, reason: &str) {
        self.record_event(
            Some(fiber),
            "policy.denied",
            json!({ "capability": capability, "reason": reason }),
        );
    }

    /// Phase 6：EvolutionGate 放行审计（事实层，可观察/可撤销依据）。
    /// 记录一次「学习替代人工审批」——Learning ≠ Authority 的可审计证据。
    pub fn record_evolution_gate(&mut self, fiber: FiberId, capability: &str, action: &str) {
        self.record_event(
            Some(fiber),
            "evolution.gate_approved",
            json!({ "capability": capability, "action": action }),
        );
    }

    // ── Phase 8 · G1：Idempotency（执行去重）────────────────────────────
    /// 幂等命中：该 (context, key) 已执行过 → 返回原 Effect id。
    pub fn idempotency_hit(&self, ctx: ContextId, key: &str) -> Option<EffectId> {
        self.idempotency.get(&(ctx, key.to_string())).copied()
    }

    /// 记录一次幂等执行（emit 成功后调用）。
    pub fn record_idempotency(&mut self, ctx: ContextId, key: &str, effect: EffectId) {
        self.idempotency.insert((ctx, key.to_string()), effect);
    }

    /// 幂等重放审计（同 key 第二次执行：不产生新业务效果，仅记录）。
    pub fn record_idempotent_replay(&mut self, fiber: FiberId, key: &str) {
        self.record_event(
            Some(fiber),
            "effect.idempotent_replay",
            json!({ "key": key }),
        );
    }

    /// Phase 8 · G2：审批挂起审计（compose 层 wait_on 时调用）。
    pub fn record_approval_requested(&mut self, fiber: FiberId, capability: &str) {
        self.record_event(
            Some(fiber),
            "approval.requested",
            json!({ "capability": capability }),
        );
    }

    // ── Phase 8 · G2：Approval 显式入口（Suspension Point 的完整语义）──
    /// 审批通过：Waiting → Running（显式恢复）+ `approval.granted` 事件。
    pub fn approve(&mut self, fiber: FiberId, by: &str) -> Result<(), KernelError> {
        let st = self
            .fiber(fiber)
            .map(|f| f.state)
            .ok_or(KernelError::FiberNotFound(fiber))?;
        if st != FiberState::Waiting {
            return Err(KernelError::InvalidTransition(
                st.as_str().to_string(),
                "Running".into(),
            ));
        }
        self.resume(fiber)?;
        // 审批通过 = 门禁放行（与 relate(Approval) 恢复路径一致：approved 置位）
        if let Some(f) = self.fibers.get_mut(&fiber) {
            f.approved = true;
        }
        self.record_event(Some(fiber), "approval.granted", json!({ "by": by }));
        Ok(())
    }

    /// 审批拒绝：Waiting → Cancelled（终态）+ `approval.rejected` 事件。
    /// 已提交 Effect 的补偿由上层显式执行（与 P1 一致：取消 ≠ 自动回滚）。
    pub fn reject_approval(&mut self, fiber: FiberId, by: &str) -> Result<(), KernelError> {
        let st = self
            .fiber(fiber)
            .map(|f| f.state)
            .ok_or(KernelError::FiberNotFound(fiber))?;
        if st != FiberState::Waiting {
            return Err(KernelError::InvalidTransition(
                st.as_str().to_string(),
                "Cancelled".into(),
            ));
        }
        self.advance(fiber, FiberState::Cancelled)?;
        self.record_event(Some(fiber), "approval.rejected", json!({ "by": by }));
        Ok(())
    }

    /// 审批超时裁决：Waiting → Cancelled + `approval.expired` 审计
    /// （与 Rejected 区分，V1 tc023d 语义）。
    pub fn expire_approval(&mut self, fiber: FiberId, by: &str) -> Result<(), KernelError> {
        let st = self
            .fiber(fiber)
            .map(|f| f.state)
            .ok_or(KernelError::FiberNotFound(fiber))?;
        if st != FiberState::Waiting {
            return Err(KernelError::InvalidTransition(
                st.as_str().to_string(),
                "Cancelled".into(),
            ));
        }
        self.advance(fiber, FiberState::Cancelled)?;
        self.record_event(Some(fiber), "approval.expired", json!({ "by": by }));
        Ok(())
    }

    // ── Phase 8 · G3：Crash Recovery（Interrupted 恢复语义）─────────────
    /// 崩溃标记：Running → Interrupted + `fiber.interrupted` 事件。
    pub fn mark_interrupted(&mut self, fiber: FiberId) -> Result<(), KernelError> {
        self.advance(fiber, FiberState::Interrupted)?;
        self.record_event(Some(fiber), "fiber.interrupted", json!({}));
        Ok(())
    }

    /// 恢复：Interrupted → Running + `fiber.recovered` 事件（resume 继续执行）。
    pub fn recover(&mut self, fiber: FiberId) -> Result<(), KernelError> {
        self.advance(fiber, FiberState::Running)?;
        self.record_event(Some(fiber), "fiber.recovered", json!({}));
        Ok(())
    }

    /// 放弃恢复：Interrupted → Failed（进入补偿路径）。
    pub fn abandon(&mut self, fiber: FiberId) -> Result<(), KernelError> {
        self.advance(fiber, FiberState::Failed)
    }

    pub fn mark_effect_compensated(&mut self, id: EffectId) {
        let fiber = self.effects.get(&id).map(|e| e.fiber);
        if let Some(e) = self.effects.get_mut(&id) {
            e.status = EffectState::Compensated;
        }
        self.record_event(
            fiber,
            "effect.compensated",
            json!({ "effect": format!("{id}") }),
        );
    }

    pub fn mark_effect_compensation_failed(&mut self, id: EffectId) {
        let fiber = self.effects.get(&id).map(|e| e.fiber);
        if let Some(e) = self.effects.get_mut(&id) {
            e.status = EffectState::CompensationFailed;
        }
        self.record_event(
            fiber,
            "effect.compensation_failed",
            json!({ "effect": format!("{id}") }),
        );
    }

    // ── 核心动作 ─────────────────────────────────────────────────────────

    /// 创建 Context（可嵌套：parent 决定作用域层级与权限继承）。
    pub fn create_context(
        &mut self,
        parent: Option<ContextId>,
        scope: Scope,
        entity_id: impl Into<String>,
    ) -> Result<ContextId, KernelError> {
        if let Some(p) = parent {
            let pc = self
                .contexts
                .get(&p)
                .ok_or(KernelError::ContextNotFound(p))?;
            if pc.state == ContextState::Disposed {
                return Err(KernelError::ContextDisposed(p));
            }
        }
        self.next_ctx += 1;
        let id = ContextId(self.next_ctx);
        let now = self.now();
        self.contexts
            .insert(id, Context::new(id, parent, scope, entity_id.into(), now));
        Ok(id)
    }

    /// 在 Context 中派生一个 Fiber（任务/Agent/工具的统一运行实例）。
    pub fn spawn(
        &mut self,
        context: ContextId,
        parent: Option<FiberId>,
        task: &str,
        input: Value,
    ) -> Result<FiberId, KernelError> {
        let ctx = self
            .contexts
            .get(&context)
            .ok_or(KernelError::ContextNotFound(context))?;
        if ctx.state == ContextState::Disposed {
            return Err(KernelError::ContextDisposed(context));
        }
        if !ctx.is_current(self.now()) {
            return Err(KernelError::ContextExpired(context));
        }
        self.next_fiber += 1;
        let id = FiberId(self.next_fiber);
        self.fibers
            .insert(id, Fiber::new(id, context, parent, task, input, self.now()));
        Ok(id)
    }

    /// 提交一个 Effect：Fiber 必须 RUNNING；记录到其 Ledger（补偿依据）。
    pub fn emit(&mut self, fiber: FiberId, mut effect: Effect) -> Result<EffectId, KernelError> {
        let f = self
            .fibers
            .get_mut(&fiber)
            .ok_or(KernelError::FiberNotFound(fiber))?;
        if f.state != FiberState::Running {
            return Err(KernelError::FiberNotRunning("emit".into()));
        }
        self.next_effect += 1;
        let id = EffectId(self.next_effect);
        effect.id = id;
        effect.fiber = fiber;
        effect.status = EffectState::Committed; // Phase 0：emit 即提交（执行由上层完成）
        let ctx = f.context;
        f.ledger.record(id);
        f.output = Some(effect.output.clone().unwrap_or(Value::Null));
        self.effects.insert(id, effect);
        self.record_event(
            Some(fiber),
            "effect.committed",
            json!({ "effect": format!("{id}") }),
        );
        let _ = ctx;
        Ok(id)
    }

    /// 记录一条 Relation（外部世界事件进入点；Effect 的副作用投影在 Phase 1 原子化）。
    /// 记录一条 Relation（外部世界事件进入点；Effect 的副作用投影在 Phase 1 原子化）。
    /// 若存在 WAITING 且 `wait_for` 匹配该 Relation 的 Fiber，自动恢复（resume）。
    pub fn relate(&mut self, mut relation: Relation) -> Result<RelationId, KernelError> {
        if !self.contexts.contains_key(&relation.context) {
            return Err(KernelError::ContextNotFound(relation.context));
        }
        self.next_relation += 1;
        let id = RelationId(self.next_relation);
        relation.id = id;
        self.relations.insert(id, relation.clone());
        self.record_event(
            None,
            "relation.created",
            json!({ "relation": format!("{id}") }),
        );
        // 等待匹配：WAITING fiber 的 wait_for 命中 → 自动恢复（审批/定时器/外部事件统一入口）
        let waiting: Vec<FiberId> = self
            .fibers
            .iter()
            .filter(|(_, f)| f.state == FiberState::Waiting)
            .filter(|(_, f)| {
                f.wait_for
                    .as_ref()
                    .map(|w| w.matches(&relation))
                    .unwrap_or(false)
            })
            .map(|(id, _)| *id)
            .collect();
        for f in waiting {
            if let Some(fb) = self.fibers.get_mut(&f) {
                fb.approved = true; // 恢复信号 = 审批门禁通过（组合层据此放行）
            }
            let _ = self.advance(f, FiberState::Running);
            self.record_event(Some(f), "fiber.resumed", json!({ "by": format!("{id}") }));
            // G4：审批类恢复必须落 granted 审计（外部事件入口与 k.approve 同源可回放；
            // by = 关系 source，即审批人/外部系统）。
            if relation.predicate == "Approval" {
                self.record_event(
                    Some(f),
                    "approval.granted",
                    json!({ "by": relation.source.clone() }),
                );
            }
        }
        Ok(id)
    }

    /// 闭合一条关系的时间窗（valid_until = at；as_of 时间旅行的数据基础）。
    pub fn expire_relation(&mut self, id: RelationId, at: i64) -> Result<(), KernelError> {
        let r = self
            .relations
            .get_mut(&id)
            .ok_or(KernelError::RelationNotFound(id))?;
        r.valid_until = Some(at);
        Ok(())
    }

    /// 声明 Fiber 的等待模式（进入 WAITING 前调用；恢复信号统一经 relate）。
    pub fn wait_on(&mut self, fiber: FiberId, pattern: WaitPattern) -> Result<(), KernelError> {
        let f = self
            .fibers
            .get_mut(&fiber)
            .ok_or(KernelError::FiberNotFound(fiber))?;
        f.wait_for = Some(pattern);
        Ok(())
    }

    /// 状态推进（scheduler/lifecycle 使用；非法转换返回错误，绝不 panic）。
    pub fn advance(&mut self, fiber: FiberId, to: FiberState) -> Result<(), KernelError> {
        self.fibers
            .get_mut(&fiber)
            .ok_or(KernelError::FiberNotFound(fiber))?
            .transition(to)
    }

    /// 显式恢复一个 WAITING / PAUSED Fiber（Phase 1 将由 relate 模式匹配自动触发）。
    pub fn resume(&mut self, fiber: FiberId) -> Result<(), KernelError> {
        let f = self
            .fibers
            .get_mut(&fiber)
            .ok_or(KernelError::FiberNotFound(fiber))?;
        f.transition(FiberState::Running)
    }

    /// 销毁单个 Fiber（终止但**不补偿**——P1 的补偿由 FAILED→COMPENSATING 路径触发）。
    pub fn dispose_fiber(&mut self, fiber: FiberId) -> Result<(), KernelError> {
        // 先收集子 Fiber（不可变借），再递归销毁（避免与可变借冲突）
        let children: Vec<FiberId> = self
            .fibers
            .values()
            .filter(|c| c.parent == Some(fiber))
            .map(|c| c.id)
            .collect();
        for c in children {
            let _ = self.dispose_fiber(c);
        }
        let f = self
            .fibers
            .get_mut(&fiber)
            .ok_or(KernelError::FiberNotFound(fiber))?;
        f.state = FiberState::Completed; // 显式销毁视为完成（不触发补偿）
        Ok(())
    }

    /// 销毁 Context（P2）：递归终止其全部 Fiber 与子 Context，**不隐式补偿业务副作用**。
    pub fn dispose_context(&mut self, context: ContextId) -> Result<(), KernelError> {
        let children: Vec<ContextId> = self
            .contexts
            .values()
            .filter(|c| c.parent == Some(context))
            .map(|c| c.id)
            .collect();
        for c in children {
            let _ = self.dispose_context(c);
        }
        let fibers: Vec<FiberId> = self
            .fibers
            .values()
            .filter(|f| f.context == context)
            .map(|f| f.id)
            .collect();
        for f in fibers {
            let _ = self.dispose_fiber(f);
        }
        let ctx = self
            .contexts
            .get_mut(&context)
            .ok_or(KernelError::ContextNotFound(context))?;
        ctx.transition(ContextState::Disposing)?;
        ctx.transition(ContextState::Disposed)
    }

    // ── 查询（Phase 0 最小集） ───────────────────────────────────────────

    pub fn context(&self, id: ContextId) -> Option<&Context> {
        self.contexts.get(&id)
    }
    pub fn fiber(&self, id: FiberId) -> Option<&Fiber> {
        self.fibers.get(&id)
    }
    pub fn effect(&self, id: EffectId) -> Option<&Effect> {
        self.effects.get(&id)
    }
    pub fn relations(&self) -> impl Iterator<Item = &Relation> {
        self.relations.values()
    }
    pub fn fibers_of(&self, context: ContextId) -> Vec<FiberId> {
        self.fibers
            .values()
            .filter(|f| f.context == context)
            .map(|f| f.id)
            .collect()
    }
    pub fn all_contexts(&self) -> impl Iterator<Item = &Context> {
        self.contexts.values()
    }
    pub fn all_fibers(&self) -> impl Iterator<Item = &Fiber> {
        self.fibers.values()
    }
    pub fn all_effects(&self) -> impl Iterator<Item = &Effect> {
        self.effects.values()
    }
    pub fn all_relations(&self) -> impl Iterator<Item = &Relation> {
        self.relations.values()
    }

    // ── 恢复辅助（persistence::restore 使用；id 计数器随最大 id 推进） ──
    pub fn restore_context(&mut self, ctx: Context) {
        self.next_ctx = self.next_ctx.max(ctx.id.0 + 1);
        self.contexts.insert(ctx.id, ctx);
    }
    pub fn restore_fiber(&mut self, f: Fiber) {
        let mut f = f;
        // Phase 8 · G3：崩溃残留 Running → Interrupted（V1 v0_8 语义移植）
        if f.state == FiberState::Running {
            f.state = FiberState::Interrupted;
        }
        self.next_fiber = self.next_fiber.max(f.id.0 + 1);
        self.fibers.insert(f.id, f);
    }
    pub fn restore_effect(&mut self, e: Effect) {
        self.next_effect = self.next_effect.max(e.id.0 + 1);
        self.effects.insert(e.id, e);
    }
    pub fn restore_relation(&mut self, r: Relation) {
        self.next_relation = self.next_relation.max(r.id.0 + 1);
        self.relations.insert(r.id, r);
    }
    pub fn restore_event(&mut self, ev: KernelEvent) {
        self.events.push(ev);
    }

    /// 某 Fiber 的已提交 Effect（按提交顺序）。
    pub fn committed_effects(&self, fiber: FiberId) -> Vec<&Effect> {
        self.fibers
            .get(&fiber)
            .map(|f| {
                f.ledger
                    .committed()
                    .iter()
                    .filter_map(|id| self.effects.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// 事实层事件（审计记录：effect 提交/补偿、relation 创建、fiber 恢复等）。
#[derive(Debug, Clone)]
pub struct KernelEvent {
    pub context: Option<ContextId>,
    pub fiber: Option<FiberId>,
    pub ty: String,
    pub payload: Value,
    pub timestamp: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 造一个 RUNNING fiber 的便捷路径。
    fn running_fiber(k: &mut Kernel) -> (ContextId, FiberId) {
        let ctx = k.create_context(None, Scope::World, "w").expect("ctx");
        let f = k.spawn(ctx, None, "demo", json!({})).expect("spawn");
        k.advance(f, FiberState::Starting).unwrap();
        k.advance(f, FiberState::Ready).unwrap();
        k.advance(f, FiberState::Running).unwrap();
        (ctx, f)
    }

    // ── P2：Context 销毁终止 Fiber，不隐式补偿 ─────────────────────────────
    #[test]
    fn context_dispose_terminates_fibers_without_compensation() {
        let mut k = Kernel::new();
        let (ctx, f) = running_fiber(&mut k);
        // 提交一个可补偿 Effect（若 dispose 触发补偿，这里会暴露）
        let e = Effect::new(
            "CreateOrder",
            json!({"amount": 100}),
            Reversibility::Compensatable,
            Some(Compensation {
                action: "CancelOrder".into(),
                payload: json!({"id": 1}),
            }),
            now_secs(),
        );
        k.emit(f, e).expect("emit");

        k.dispose_context(ctx).expect("dispose");
        assert_eq!(k.context(ctx).unwrap().state, ContextState::Disposed);
        assert_eq!(k.fiber(f).unwrap().state, FiberState::Completed);
        // Effect 仍为 Committed——Context 销毁不隐式补偿业务副作用（P2）
        let committed = k.committed_effects(f);
        assert_eq!(committed.len(), 1);
        assert_eq!(committed[0].status, EffectState::Committed);
    }

    // ── 层级：父 Context 销毁递归终止子 Context 的 Fiber ──────────────────
    #[test]
    fn nested_context_dispose_recurses() {
        let mut k = Kernel::new();
        let world = k.create_context(None, Scope::World, "w").unwrap();
        let tenant = k
            .create_context(Some(world), Scope::Tenant, "acme")
            .unwrap();
        let (_, f) = {
            let ctx = k.create_context(Some(tenant), Scope::Agent, "cfo").unwrap();
            let f = k.spawn(ctx, None, "analyze", json!({})).unwrap();
            (ctx, f)
        };
        k.dispose_context(world).unwrap();
        assert_eq!(k.context(tenant).unwrap().state, ContextState::Disposed);
        assert_eq!(k.fiber(f).unwrap().state, FiberState::Completed);
    }

    // ── Fiber 状态机：合法路径 + 非法转换拒绝 ─────────────────────────────
    #[test]
    fn fiber_state_machine_valid_and_invalid() {
        let mut k = Kernel::new();
        let (_, f) = running_fiber(&mut k);
        // 合法：Running → Failed → Compensating → Compensated
        k.advance(f, FiberState::Failed).unwrap();
        k.advance(f, FiberState::Compensating).unwrap();
        k.advance(f, FiberState::Compensated).unwrap();
        // 非法：Compensated 不能回 Running
        assert!(matches!(
            k.advance(f, FiberState::Running),
            Err(KernelError::InvalidTransition(_, _))
        ));
        // 非法：Failed 不能直接 Completed
        let (_, f2) = running_fiber(&mut k);
        k.advance(f2, FiberState::Failed).unwrap();
        assert!(matches!(
            k.advance(f2, FiberState::Completed),
            Err(KernelError::InvalidTransition(_, _))
        ));
    }

    // ── P1：Ledger 逆序补偿顺序（C⁻¹ → B⁻¹ → A⁻¹） ────────────────────────
    #[test]
    fn effect_ledger_reverse_compensation_order() {
        let mut ledger = EffectLedger::new();
        for i in 1..=3 {
            ledger.record(EffectId(i));
        }
        let order: Vec<EffectId> = ledger.reverse_order().copied().collect();
        assert_eq!(order, vec![EffectId(3), EffectId(2), EffectId(1)]);
    }

    // ── Reversibility 分类：仅 Reversible/Compensatable 进入补偿 ──────────
    #[test]
    fn reversibility_classification() {
        assert!(Effect::new("a", json!({}), Reversibility::Reversible, None, 0).is_compensatable());
        assert!(
            Effect::new("a", json!({}), Reversibility::Compensatable, None, 0).is_compensatable()
        );
        assert!(
            !Effect::new("a", json!({}), Reversibility::Idempotent, None, 0).is_compensatable()
        );
        assert!(
            !Effect::new("a", json!({}), Reversibility::Irreversible, None, 0).is_compensatable()
        );
    }

    // ── P3：Relation 当前有效过滤（valid_from/valid_until 边界） ──────────
    #[test]
    fn relation_is_current_boundaries() {
        let mut k = Kernel::new();
        let ctx = k.create_context(None, Scope::World, "w").unwrap();
        let mut r = Relation::new(ctx, "Customer", "owns", "Account", 100);
        assert!(r.is_current(100));
        assert!(!r.is_current(99), "valid_from 之前无效");
        r.expire(200);
        assert!(r.is_current(199));
        assert!(!r.is_current(200), "valid_until 处失效");
    }

    // ── emit 前置条件：非 RUNNING 拒绝；WAITING 经 resume 恢复 ────────────
    #[test]
    fn emit_requires_running_and_resume_recovers_waiting() {
        let mut k = Kernel::new();
        let (ctx, f) = running_fiber(&mut k);
        // Running → Waiting
        k.advance(f, FiberState::Waiting).unwrap();
        // WAITING 时 emit 拒绝
        let e = Effect::new("x", json!({}), Reversibility::Reversible, None, now_secs());
        assert!(matches!(k.emit(f, e), Err(KernelError::FiberNotRunning(_))));
        // resume → Running → 可 emit
        k.resume(f).unwrap();
        let e2 = Effect::new("x", json!({}), Reversibility::Reversible, None, now_secs());
        let eid = k.emit(f, e2).expect("emit after resume");
        assert!(k.effect(eid).is_some());
        let _ = ctx;
    }

    // ── P3 硬约束：过期 Context 不能 spawn 新 Fiber（注入时钟验证） ────────
    #[test]
    fn expired_context_blocks_spawn() {
        let mut k = Kernel::with_clock(|| 1000); // 固定时钟 now=1000
        let ctx = k.create_context(None, Scope::Tenant, "acme").unwrap();
        k.contexts.get_mut(&ctx).unwrap().valid_until = Some(500); // 500 过期
        assert!(matches!(
            k.spawn(ctx, None, "t", json!({})),
            Err(KernelError::ContextExpired(_))
        ));
        // 有效窗口内可 spawn
        k.contexts.get_mut(&ctx).unwrap().valid_until = Some(2000);
        assert!(k.spawn(ctx, None, "t", json!({})).is_ok());
    }
}
