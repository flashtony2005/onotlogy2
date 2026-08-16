//! WorldCommand（P5.3）—— 事务化世界变更：EveryWorldChange → Event（契约 P5）。
//!
//! 语义（契约 projection-v1.md §1.5 拍板）：
//! `Command → validate → transaction { append(WorldEvent); apply/persist } → Projection → Kernel`
//! 对象/关系写入与 world 事件**同一事务**落库——Storage 不再存在「裸事实」
//! （裸写对象在投影世界里不存在，P7 测试已钉）。
//! 事件载荷按契约第 2 节：ObjectCreated `refs=[id], input={type,data}`；
//! RelationLinked `refs=[source,target], input={edge}`；run_id=None（事实不依赖 Run）。

use libsql::{Transaction, params};
use serde_json::json;

use super::*;
use oaip_trace::{TraceEvent, TraceKind};

/// 世界变更命令（P5 的唯一世界写入口）。
#[derive(Debug, Clone)]
pub enum WorldCommand {
    CreateObject {
        object_type: String,
        id: String,
        data: Value,
    },
    UpdateObject {
        id: String,
        data: Value,
    },
    DeleteObject {
        id: String,
    },
    LinkRelation {
        source: String,
        edge: String,
        target: String,
    },
    UnlinkRelation {
        source: String,
        edge: String,
        target: String,
    },
}

impl WorldCommand {
    /// 事件载荷（契约第 2 节）。
    fn world_event(&self) -> TraceEvent {
        let (ty, refs, input) = match self {
            WorldCommand::CreateObject {
                object_type,
                id,
                data,
            } => (
                "ObjectCreated",
                vec![id.clone()],
                Some(json!({ "type": object_type, "data": data })),
            ),
            WorldCommand::UpdateObject { id, data } => (
                "ObjectUpdated",
                vec![id.clone()],
                Some(json!({ "data": data })),
            ),
            WorldCommand::DeleteObject { id } => ("ObjectDeleted", vec![id.clone()], None),
            WorldCommand::LinkRelation {
                source,
                edge,
                target,
            } => (
                "RelationLinked",
                vec![source.clone(), target.clone()],
                Some(json!({ "edge": edge })),
            ),
            WorldCommand::UnlinkRelation {
                source,
                edge,
                target,
            } => (
                "RelationUnlinked",
                vec![source.clone(), target.clone()],
                Some(json!({ "edge": edge })),
            ),
        };
        TraceEvent {
            id: oaip_trace::new_trace_id(),
            kind: TraceKind::World,
            event_type: ty.to_string(),
            object_refs: refs,
            run_id: None, // 事实不依赖 Run（审计解耦点）
            agent_id: None,
            reasoning: None,
            input,
            output: None,
            confidence: None,
            timestamp: oaip_trace::now_ts(),
        }
    }
}

impl LibSQLStorage {
    /// 事务化应用一条世界变更：world 事件 + 对象/关系写在同一事务（P5）。
    /// 事件先插（事实先行），对象写随后，事务提交保证原子。
    pub async fn apply_world(&self, cmd: WorldCommand) -> Result<(), StorageError> {
        let tx = self.conn.transaction().await?;
        let ev = cmd.world_event();
        insert_trace_tx(&tx, &ev).await?;
        match cmd {
            WorldCommand::CreateObject {
                object_type,
                id,
                data,
            } => {
                let data_str = serde_json::to_string(&data)?;
                tx.execute(
                    "INSERT INTO wb_ontology_object (id, type, data, version) VALUES (?1,?2,?3,1)",
                    params![id.as_str(), object_type.as_str(), data_str.as_str()],
                )
                .await?;
            }
            WorldCommand::UpdateObject { id, data } => {
                let data_str = serde_json::to_string(&data)?;
                tx.execute(
                    "UPDATE wb_ontology_object SET data=?1, version=version+1, \
                     update_time=CURRENT_TIMESTAMP WHERE id=?2 AND is_delete=0",
                    params![data_str.as_str(), id.as_str()],
                )
                .await?;
            }
            WorldCommand::DeleteObject { id } => {
                tx.execute(
                    "UPDATE wb_ontology_object SET is_delete=1, update_time=CURRENT_TIMESTAMP \
                     WHERE id=?1 AND is_delete=0",
                    params![id.as_str()],
                )
                .await?;
            }
            WorldCommand::LinkRelation {
                source,
                edge,
                target,
            } => {
                // 事务内「检测 + 插入」：杜绝 TOCTOU 双插（与 relations::link 同语义）
                let mut dup = tx
                    .query(
                        "SELECT id FROM wb_relation_edge WHERE source=?1 AND relation=?2 AND target=?3 AND is_delete=0",
                        params![source.as_str(), edge.as_str(), target.as_str()],
                    )
                    .await?;
                if dup.next().await?.is_some() {
                    let _ = tx.rollback().await;
                    return Err(StorageError::Constraint {
                        code: "WB2007",
                        message: format!("重复关系边 {source}-{edge}->{target}"),
                    });
                }
                let id = format!(
                    "rel_{}_{}",
                    oaip_trace::now_ts(),
                    REL_SEQ.fetch_add(1, Ordering::SeqCst)
                );
                tx.execute(
                    "INSERT INTO wb_relation_edge (id, relation, source, target) VALUES (?1,?2,?3,?4)",
                    params![id.as_str(), edge.as_str(), source.as_str(), target.as_str()],
                )
                .await?;
            }
            WorldCommand::UnlinkRelation {
                source,
                edge,
                target,
            } => {
                tx.execute(
                    "UPDATE wb_relation_edge SET is_delete=1, update_time=CURRENT_TIMESTAMP \
                     WHERE source=?1 AND relation=?2 AND target=?3 AND is_delete=0",
                    params![source.as_str(), edge.as_str(), target.as_str()],
                )
                .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    /// 批量应用（演示种子/迁移脚本用；逐条事务，任一条失败即中断）。
    pub async fn apply_world_many(&self, cmds: &[WorldCommand]) -> Result<(), StorageError> {
        for cmd in cmds {
            self.apply_world(cmd.clone()).await?;
        }
        Ok(())
    }
}

/// 事务内插入 trace 事件（append_trace 的事务版本）。
async fn insert_trace_tx(tx: &Transaction, ev: &TraceEvent) -> Result<(), StorageError> {
    let obj_refs = serde_json::to_string(&ev.object_refs)?;
    let input = ev.input.as_ref().map(serde_json::to_string).transpose()?;
    let output = ev.output.as_ref().map(serde_json::to_string).transpose()?;
    tx.execute(
        "INSERT INTO wb_trace_event \
         (id, kind, event_type, object_refs, run_id, agent_id, reasoning, input, output, confidence, timestamp) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            ev.id.clone(),
            ev.kind.as_str(),
            ev.event_type.clone(),
            obj_refs,
            ev.run_id.clone(),
            ev.agent_id.clone(),
            ev.reasoning.clone(),
            input,
            output,
            ev.confidence,
            ev.timestamp.clone()
        ],
    )
    .await?;
    Ok(())
}
