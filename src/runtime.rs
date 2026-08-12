//! 运行时可观察性 —— TraceEvent + EventBus + RuntimeKernel（对应 REQ-007 / REQ-008）
//!
//! TraceEvent 是「可解释性根基」：world 事件由 Object CRUD 产生，绑定 object_refs；
//! EventBus 在落库同时向订阅者发布，保证「写入与发布一致」。

use crate::agent::WorldSnapshot;
use crate::storage::{
    LibSQLStorage, ObjectEngine, ObjectRecord, RelationEngine, RelationRecord, StorageError,
    TraceQuery,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 事件大类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceKind {
    World,
    Cognitive,
}

impl TraceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            TraceKind::World => "world",
            TraceKind::Cognitive => "cognitive",
        }
    }
}

impl FromStr for TraceKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "world" => Ok(TraceKind::World),
            "cognitive" => Ok(TraceKind::Cognitive),
            other => Err(format!("未知 TraceKind: {other}")),
        }
    }
}

/// 统一事件（world / cognitive 同源）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraceEvent {
    pub id: String,
    pub kind: TraceKind,
    pub event_type: String,
    /// 绑定的 Ontology 对象ID数组——可解释性根基，禁止为空（TC-007-b）。
    pub object_refs: Vec<String>,
    pub run_id: Option<String>,
    pub agent_id: Option<String>,
    pub reasoning: Option<String>,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub confidence: Option<f64>,
    pub timestamp: String,
}

static TRACE_SEQ: AtomicU64 = AtomicU64::new(0);

fn now_ts() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", d.as_micros())
}

fn new_trace_id() -> String {
    format!(
        "tr_{}_{}",
        now_ts(),
        TRACE_SEQ.fetch_add(1, Ordering::SeqCst)
    )
}

impl TraceEvent {
    /// 构造 world 事件（ObjectCreated/Updated/Deleted）。
    pub fn world(event_type: &str, object_ref: &str) -> Self {
        Self::world_with_refs(event_type, &[object_ref.to_string()])
    }

    /// 构造 world 事件（支持多对象绑定，如 Relation 的 [src, dst]）。
    /// [强制] object_refs 非空，否则落库被 WB2006 拦截（TC-007-b）。
    pub fn world_with_refs(event_type: &str, refs: &[String]) -> Self {
        Self {
            id: new_trace_id(),
            kind: TraceKind::World,
            event_type: event_type.to_string(),
            object_refs: refs.to_vec(),
            run_id: None,
            agent_id: None,
            reasoning: None,
            input: None,
            output: None,
            confidence: None,
            timestamp: now_ts(),
        }
    }
}

/// 极简发布/订阅总线（多订阅者，落库后发布）。
pub struct EventBus {
    tx: tokio::sync::broadcast::Sender<TraceEvent>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel(1024);
        Self { tx }
    }
    /// 发布事件（接收者满则丢弃，不阻塞落库）。
    pub fn publish(&self, ev: TraceEvent) {
        let _ = self.tx.send(ev);
    }
    /// 订阅（新订阅者仅收到后续事件）。
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<TraceEvent> {
        self.tx.subscribe()
    }
}

/// 运行时内核：把 ObjectEngine 与 EventBus 串起来。
/// 每个 Object CRUD 同时：① 落库对象 ② 产生 world TraceEvent ③ 落库事件 ④ 发布到总线。
pub struct RuntimeKernel {
    storage: LibSQLStorage,
    bus: EventBus,
}

impl RuntimeKernel {
    pub fn new(storage: LibSQLStorage) -> Self {
        Self {
            storage,
            bus: EventBus::new(),
        }
    }

    pub fn event_bus(&self) -> &EventBus {
        &self.bus
    }

    pub async fn create_object(
        &self,
        object_type: &str,
        id: &str,
        data: Value,
    ) -> Result<ObjectRecord, StorageError> {
        let rec = self.storage.create(object_type, id, data).await?;
        self.emit(TraceEvent::world("ObjectCreated", id)).await?;
        Ok(rec)
    }

    pub async fn update_object(&self, id: &str, data: Value) -> Result<ObjectRecord, StorageError> {
        let rec = self.storage.update(id, data).await?;
        self.emit(TraceEvent::world("ObjectUpdated", &rec.id))
            .await?;
        Ok(rec)
    }

    /// 按语义 ID 读取单个对象（供 Planner 幂等能力判断）。
    pub async fn get_object(&self, id: &str) -> Result<Option<ObjectRecord>, StorageError> {
        self.storage.get(id).await
    }

    pub async fn delete_object(&self, id: &str) -> Result<(), StorageError> {
        self.storage.delete(id).await?;
        self.emit(TraceEvent::world("ObjectDeleted", id)).await?;
        Ok(())
    }

    /// 落库事件并发布（写入与发布一致，TC-008）。
    async fn emit(&self, ev: TraceEvent) -> Result<(), StorageError> {
        self.storage.append_trace(&ev).await?;
        self.bus.publish(ev);
        Ok(())
    }

    /// 发一条 cognitive 认知事件（绑定 run/agent/object_refs），落库并发布（与 world 事件同源）。
    /// [强制] object_refs 非空，否则落库被 WB2006 拦截（TC-007-b 同源约束）。
    #[allow(clippy::too_many_arguments)]
    pub async fn emit_cognitive(
        &self,
        event_type: &str,
        run_id: &str,
        agent_id: &str,
        object_refs: &[String],
        reasoning: Option<&str>,
        input: Option<Value>,
        output: Option<Value>,
        confidence: Option<f64>,
    ) -> Result<(), StorageError> {
        let ev = TraceEvent {
            id: new_trace_id(),
            kind: TraceKind::Cognitive,
            event_type: event_type.to_string(),
            object_refs: object_refs.to_vec(),
            run_id: Some(run_id.to_string()),
            agent_id: Some(agent_id.to_string()),
            reasoning: reasoning.map(|s| s.to_string()),
            input,
            output,
            confidence,
            timestamp: now_ts(),
        };
        self.emit(ev).await
    }

    /// 按条件查询 Trace 事件（认知/世界同源），供 timeline 与 WS 重连补齐（after_id）。
    pub async fn list_events(&self, q: TraceQuery) -> Result<Vec<TraceEvent>, StorageError> {
        self.storage.list_events(q).await
    }

    /// 按 ID 取单条 Trace 事件（缺失 → None，上层映射 WB2002）。
    pub async fn get_trace(&self, id: &str) -> Result<Option<TraceEvent>, StorageError> {
        self.storage.get_trace(id).await
    }

    /// 列出已落库的 world 事件（对应 `oaip event list`）。
    pub async fn list_world_events(&self) -> Result<Vec<TraceEvent>, StorageError> {
        self.storage.list_world_events().await
    }

    /// 世界模型快照：全量对象 + 关系，作为认知引擎的最小必要上下文（prd 4.4）。
    pub async fn snapshot_world(&self) -> Result<WorldSnapshot, StorageError> {
        let objs = self.storage.list_all_objects().await?;
        let rels = self.storage.list_all_relations().await?;
        let objects: Vec<Value> = objs
            .into_iter()
            .map(|o| json!({"id": o.id, "type": o.r#type, "data": o.data}))
            .collect();
        let relations: Vec<Value> = rels
            .into_iter()
            .map(|r| json!({"source": r.source, "relation": r.relation, "target": r.target}))
            .collect();
        Ok(WorldSnapshot { objects, relations })
    }

    /// 按类型查询对象（对应 `oaip object list`）。
    pub async fn query_objects(
        &self,
        object_type: &str,
    ) -> Result<Vec<ObjectRecord>, StorageError> {
        self.storage.query(object_type).await
    }

    // ---- V0.2 World Model：Relation + 领域 Event ----

    /// 建关系边，并产出 world 事件 RelationLinked 绑定 [src, dst]。
    pub async fn link_relation(
        &self,
        source_id: &str,
        edge_type: &str,
        target_id: &str,
    ) -> Result<RelationRecord, StorageError> {
        let rec = self
            .storage
            .link(source_id, edge_type, target_id, None)
            .await?;
        self.emit(TraceEvent::world_with_refs(
            "RelationLinked",
            &[source_id.to_string(), target_id.to_string()],
        ))
        .await?;
        Ok(rec)
    }

    /// 查询从某对象出发的所有出边。
    pub async fn neighbors(&self, source_id: &str) -> Result<Vec<RelationRecord>, StorageError> {
        self.storage.neighbors(source_id).await
    }

    /// 按边类型查询全部边。
    pub async fn query_relations(
        &self,
        edge_type: &str,
    ) -> Result<Vec<RelationRecord>, StorageError> {
        self.storage.query_by_edge(edge_type).await
    }

    /// 发出领域业务事件（如 PaymentLate），经 EventBus 发布 world TraceEvent。
    pub async fn emit_domain_event(
        &self,
        event_type: &str,
        refs: &[&str],
    ) -> Result<(), StorageError> {
        let refs: Vec<String> = refs.iter().map(|s| s.to_string()).collect();
        let ev = TraceEvent::world_with_refs(event_type, &refs);
        self.emit(ev).await
    }

    // ---- V0.5 Agent 自治：记忆沉淀（Memory = Event 积累；BL-008） ----

    /// 沉淀一条记忆（upsert，hits 累加）。
    pub async fn record_memory(
        &self,
        label: &str,
        payload: Value,
    ) -> Result<crate::storage::MemoryRecord, StorageError> {
        self.storage.record_memory(label, payload).await
    }

    /// 按标签召回单条记忆。
    pub async fn recall_memory(
        &self,
        label: &str,
    ) -> Result<Option<crate::storage::MemoryRecord>, StorageError> {
        self.storage.recall_memory(label).await
    }

    /// 召回全部记忆（hits 降序）。
    pub async fn recall_all(&self) -> Result<Vec<crate::storage::MemoryRecord>, StorageError> {
        self.storage.recall_all().await
    }

    // ---- Evolution Engine（BL-003 · REQ-017）：Experience → Knowledge，不修改 Policy ----

    /// 运行一轮 Evolution：把当前 `wb_memory` 经验提炼为 Pattern→Proposal→Validation→LearnedKnowledge。
    ///
    /// - 幂等：按稳定 id upsert，可重复调用不重复产出知识（TC-025f）；
    /// - 不修改 Policy、不绕过 Human Approval（Learn≠Permission）；
    /// - `run_id` 作为证据注入每条 Pattern/Proposal/LearnedKnowledge 的 `run_ids`。
    pub async fn run_evolution(
        &self,
        run_id: &str,
    ) -> Result<crate::evolution::EvolutionState, StorageError> {
        // 1) Experience：读取记忆（Experience = V0.5 的 wb_memory）
        let memories = self.storage.recall_all().await?;
        let experiences: Vec<crate::evolution::Experience> = memories
            .into_iter()
            .map(|m| crate::evolution::Experience {
                label: m.label.clone(),
                hits: m.hits,
            })
            .collect();

        // 2) Pattern：挖掘高频模式
        let patterns = crate::evolution::mine_patterns(&experiences);
        for p in &patterns {
            self.storage.save_pattern(p, &[run_id.to_string()]).await?;
        }

        // 3) Proposal + 4) Validation
        let mut proposals = Vec::new();
        let mut learned = Vec::new();
        for p in &patterns {
            let mut prop = crate::evolution::propose(p);
            prop.run_ids =
                crate::evolution::merge_run_ids(prop.run_ids.clone(), &[run_id.to_string()]);
            let status = crate::evolution::validate(&prop);
            prop.status = status;
            let prop_clone = prop.clone();
            self.storage.save_proposal(&prop_clone).await?;
            proposals.push(prop_clone.clone());

            // 5) LearnedKnowledge：仅 Validated 才沉淀
            if status == crate::evolution::ProposalStatus::Validated {
                let lk = crate::evolution::LearnedKnowledge {
                    id: crate::evolution::learned_id_of(&prop.target),
                    kind: prop.kind,
                    action: prop.target.clone(),
                    confidence: prop.confidence,
                    proposal_id: prop.id.clone(),
                    run_ids: prop.run_ids.clone(),
                    created_at: now_ts(),
                };
                let lk_clone = lk.clone();
                self.storage.save_learned(&lk_clone).await?;
                learned.push(lk_clone);
            } else {
                // Proposal 未通过/降级时，同步撤销已沉淀的 LearnedKnowledge，避免悬空引用。
                let learned_id = crate::evolution::learned_id_of(&prop.target);
                self.storage.delete_learned(&learned_id).await?;
            }
        }

        Ok(crate::evolution::EvolutionState {
            patterns: self.storage.list_patterns().await?,
            proposals: self.storage.list_proposals().await?,
            learned: self.storage.list_learned().await?,
        })
    }

    /// 读取当前 Evolution 聚合状态（供 HTTP API / UI）。
    pub async fn evolution_state(&self) -> Result<crate::evolution::EvolutionState, StorageError> {
        Ok(crate::evolution::EvolutionState {
            patterns: self.storage.list_patterns().await?,
            proposals: self.storage.list_proposals().await?,
            learned: self.storage.list_learned().await?,
        })
    }

    /// 撤销一条 LearnedKnowledge（可撤销承诺；幂等，不存在也返回 Ok）。
    pub async fn revoke_learned(&self, id: &str) -> Result<(), StorageError> {
        self.storage.delete_learned(id).await
    }
}
