//! WorldCommand（P5.3）—— 事务化世界变更：EveryWorldChange → Event（契约 P5）。
//!
//! 语义（契约 projection-v1.md §1.5 拍板）：
//! `Command → validate → transaction { append(WorldEvent); apply/persist } → Projection → Kernel`
//! 对象/关系写入与 world 事件**同一事务**落库——Storage 不再存在「裸事实」
//! （裸写对象在投影世界里不存在，P7 测试已钉）。
//! 事件载荷按契约第 2 节：ObjectCreated `refs=[id], input={type,data}`；
//! RelationLinked `refs=[source,target], input={edge}`；run_id=None（事实不依赖 Run）。
//! P6：`world_link_event` 是 Effect 提交点回写事件的同源构造器（契约载荷唯一实现）。

use libsql::params;
use serde_json::json;

use super::*;
use oaip_trace::{TraceEvent, TraceKind};

/// P6：RelationLinked 世界事件（契约第 2 节载荷；Effect 提交点与 LinkRelation
/// 命令共用——事件与 Kernel 关系一一对应，投影重放后状态一致）。
pub fn world_link_event(
    source: impl Into<String>,
    edge: impl Into<String>,
    target: impl Into<String>,
) -> TraceEvent {
    TraceEvent {
        id: oaip_trace::new_trace_id(),
        kind: TraceKind::World,
        event_type: "RelationLinked".to_string(),
        object_refs: vec![source.into(), target.into()],
        project_id: None,
        run_id: None, // 事实不依赖 Run（审计解耦点）
        agent_id: None,
        reasoning: None,
        input: Some(json!({ "edge": edge.into() })),
        output: None,
        confidence: None,
        timestamp: oaip_trace::now_ts(),
    }
}

/// 世界变更命令（P5 的唯一世界写入口）。
#[derive(Debug, Clone)]
pub enum WorldCommand {
    CreateObject {
        object_type: String,
        id: String,
        data: Value,
        /// Phase 8：项目归属（None = 全局；世界按项目过滤）。
        project_id: Option<String>,
    },
    UpdateObject {
        id: String,
        data: Value,
        project_id: Option<String>,
    },
    DeleteObject {
        id: String,
        project_id: Option<String>,
    },
    LinkRelation {
        source: String,
        edge: String,
        target: String,
        project_id: Option<String>,
    },
    UnlinkRelation {
        source: String,
        edge: String,
        target: String,
        project_id: Option<String>,
    },
}

impl WorldCommand {
    /// 事件载荷（契约第 2 节）。
    fn world_event(&self) -> TraceEvent {
        let (ty, refs, input, project_id) = match self {
            WorldCommand::CreateObject {
                object_type,
                id,
                data,
                project_id,
            } => (
                "ObjectCreated",
                vec![id.clone()],
                Some(json!({ "type": object_type, "data": data })),
                project_id.clone(),
            ),
            WorldCommand::UpdateObject {
                id,
                data,
                project_id,
            } => (
                "ObjectUpdated",
                vec![id.clone()],
                Some(json!({ "data": data })),
                project_id.clone(),
            ),
            WorldCommand::DeleteObject { id, project_id } => {
                ("ObjectDeleted", vec![id.clone()], None, project_id.clone())
            }
            WorldCommand::LinkRelation {
                source,
                edge,
                target,
                project_id,
            } => {
                let mut ev = world_link_event(source, edge, target);
                ev.project_id = project_id.clone();
                return ev;
            }
            WorldCommand::UnlinkRelation {
                source,
                edge,
                target,
                project_id,
            } => (
                "RelationUnlinked",
                vec![source.clone(), target.clone()],
                Some(json!({ "edge": edge })),
                project_id.clone(),
            ),
        };
        TraceEvent {
            id: oaip_trace::new_trace_id(),
            kind: TraceKind::World,
            event_type: ty.to_string(),
            object_refs: refs,
            project_id,
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
        if let Err(error) = super::events::append_trace_tx(&tx, &ev).await {
            if super::events::is_audit_chain_error(&error) {
                self.mark_audit_chain_invalid();
            }
            return Err(error);
        }
        #[cfg(test)]
        crate::crash_if_requested("world.after_event_insert");
        #[cfg(test)]
        crate::io_fault_if_requested("world.after_event_insert")?;
        match cmd {
            WorldCommand::CreateObject {
                object_type,
                id,
                data,
                project_id,
            } => {
                let data_str = serde_json::to_string(&data)?;
                tx.execute(
                    "INSERT INTO wb_ontology_object (id, type, data, version, project_id) VALUES (?1,?2,?3,1,?4)",
                    params![id.as_str(), object_type.as_str(), data_str.as_str(), project_id.as_deref()],
                )
                .await?;
            }
            WorldCommand::UpdateObject {
                id,
                data,
                project_id,
            } => {
                let data_str = serde_json::to_string(&data)?;
                tx.execute(
                    "UPDATE wb_ontology_object SET data=?1, version=version+1, \
                     update_time=CURRENT_TIMESTAMP WHERE id=?2 AND is_delete=0 \
                     AND ((project_id = ?3) OR (project_id IS NULL AND ?3 IS NULL))",
                    params![data_str.as_str(), id.as_str(), project_id.as_deref()],
                )
                .await?;
            }
            WorldCommand::DeleteObject { id, project_id } => {
                tx.execute(
                    "UPDATE wb_ontology_object SET is_delete=1, update_time=CURRENT_TIMESTAMP \
                     WHERE id=?1 AND is_delete=0 \
                     AND ((project_id = ?2) OR (project_id IS NULL AND ?2 IS NULL))",
                    params![id.as_str(), project_id.as_deref()],
                )
                .await?;
            }
            WorldCommand::LinkRelation {
                source,
                edge,
                target,
                project_id,
            } => {
                // 事务内「检测 + 插入」：杜绝 TOCTOU 双插（与 relations::link 同语义）
                let mut dup = tx
                    .query(
                        "SELECT id FROM wb_relation_edge WHERE source=?1 AND relation=?2 AND target=?3 \
                         AND is_delete=0 AND ((project_id = ?4) OR (project_id IS NULL AND ?4 IS NULL))",
                        params![source.as_str(), edge.as_str(), target.as_str(), project_id.as_deref()],
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
                    "INSERT INTO wb_relation_edge (id, relation, source, target, project_id) VALUES (?1,?2,?3,?4,?5)",
                    params![id.as_str(), edge.as_str(), source.as_str(), target.as_str(), project_id.as_deref()],
                )
                .await?;
            }
            WorldCommand::UnlinkRelation {
                source,
                edge,
                target,
                project_id,
            } => {
                tx.execute(
                    "UPDATE wb_relation_edge SET is_delete=1, update_time=CURRENT_TIMESTAMP \
                     WHERE source=?1 AND relation=?2 AND target=?3 AND is_delete=0 \
                     AND ((project_id = ?4) OR (project_id IS NULL AND ?4 IS NULL))",
                    params![
                        source.as_str(),
                        edge.as_str(),
                        target.as_str(),
                        project_id.as_deref()
                    ],
                )
                .await?;
            }
        }
        #[cfg(test)]
        crate::crash_if_requested("world.after_state_write");
        #[cfg(test)]
        crate::io_fault_if_requested("world.after_state_write")?;
        tx.commit().await?;
        self.mark_audit_chain_valid();
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
