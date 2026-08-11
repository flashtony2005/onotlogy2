//! 运行时可观察性 —— TraceEvent + EventBus + RuntimeKernel（对应 REQ-007 / REQ-008）
//!
//! TraceEvent 是「可解释性根基」：world 事件由 Object CRUD 产生，绑定 object_refs；
//! EventBus 在落库同时向订阅者发布，保证「写入与发布一致」。

use crate::storage::{
    LibSQLStorage, ObjectEngine, ObjectRecord, RelationEngine, RelationRecord, StorageError,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

    /// 列出已落库的 world 事件（对应 `oaip event list`）。
    pub async fn list_world_events(&self) -> Result<Vec<TraceEvent>, StorageError> {
        self.storage.list_world_events().await
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
}
