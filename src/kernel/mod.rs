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

use serde_json::{Value, json};
use std::collections::HashMap;

pub mod compensation;
pub mod context;
pub mod effect;
pub mod fiber;
pub mod persistence;
pub mod relation;
pub mod scheduler;
pub mod state;

pub use compensation::{CompensationEngine, Compensator, NullCompensator, RecordingCompensator};
pub use context::{Context, ContextId, ContextState, Scope};
pub use effect::{Compensation, Effect, EffectId, EffectLedger, EffectState, Reversibility};
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
    #[error("无法生成新动作：{0}")]
    Closed(String),
}

/// 当前 unix 秒（Kernel 时钟，测试可注入）。
pub(crate) fn now_secs() -> i64 {
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
        }
    }
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

    fn now(&self) -> i64 {
        (self.clock)()
    }

    /// 注入补偿执行器（P1）。
    pub fn with_compensator(mut self, c: std::sync::Arc<dyn Compensator>) -> Self {
        self.compensator = c;
        self
    }

    pub(crate) fn compensator(&self) -> std::sync::Arc<dyn Compensator> {
        self.compensator.clone()
    }

    /// 事实层事件（审计）。
    pub fn events(&self) -> &[KernelEvent] {
        &self.events
    }

    fn record_event(&mut self, fiber: Option<FiberId>, ty: &str, payload: Value) {
        let ctx = fiber.and_then(|f| self.fibers.get(&f).map(|x| x.context));
        self.events.push(KernelEvent {
            context: ctx,
            fiber,
            ty: ty.to_string(),
            payload,
            timestamp: self.now(),
        });
    }

    pub(crate) fn mark_effect_compensated(&mut self, id: EffectId) {
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

    pub(crate) fn mark_effect_compensation_failed(&mut self, id: EffectId) {
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
            let _ = self.advance(f, FiberState::Running);
            self.record_event(Some(f), "fiber.resumed", json!({ "by": format!("{id}") }));
        }
        Ok(id)
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
    pub(crate) fn restore_context(&mut self, ctx: Context) {
        self.next_ctx = self.next_ctx.max(ctx.id.0 + 1);
        self.contexts.insert(ctx.id, ctx);
    }
    pub(crate) fn restore_fiber(&mut self, f: Fiber) {
        self.next_fiber = self.next_fiber.max(f.id.0 + 1);
        self.fibers.insert(f.id, f);
    }
    pub(crate) fn restore_effect(&mut self, e: Effect) {
        self.next_effect = self.next_effect.max(e.id.0 + 1);
        self.effects.insert(e.id, e);
    }
    pub(crate) fn restore_relation(&mut self, r: Relation) {
        self.next_relation = self.next_relation.max(r.id.0 + 1);
        self.relations.insert(r.id, r);
    }
    pub(crate) fn restore_event(&mut self, ev: KernelEvent) {
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
