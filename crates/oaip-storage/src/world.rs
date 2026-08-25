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
    CompareSetStatus {
        object_type: String,
        id: String,
        project_id: Option<String>,
        expected_status: String,
        next_status: String,
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
            WorldCommand::CompareSetStatus {
                object_type,
                id,
                project_id,
                expected_status,
                next_status,
            } => (
                "ObjectUpdated",
                vec![id.clone()],
                Some(json!({
                    "type": object_type,
                    "expectedStatus": expected_status,
                    "nextStatus": next_status,
                })),
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
        let _write_txn = self.write_txn_lock.lock().await;
        // IMMEDIATE 在写入口获取数据库锁，让状态 CAS 和预算账本一样跨实例安全。
        let tx = self
            .conn
            .transaction_with_behavior(libsql::TransactionBehavior::Immediate)
            .await?;
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
            WorldCommand::CompareSetStatus {
                object_type,
                id,
                project_id,
                expected_status,
                next_status,
            } => {
                let mut rows = tx
                    .query(
                        "SELECT data FROM wb_ontology_object WHERE id=?1 AND type=?2 AND is_delete=0 \
                         AND ((project_id = ?3) OR (project_id IS NULL AND ?3 IS NULL))",
                        params![id.as_str(), object_type.as_str(), project_id.as_deref()],
                    )
                    .await?;
                let Some(row) = rows.next().await? else {
                    return Err(StorageError::NotFound(id.to_string()));
                };
                let data: String = row.get(0)?;
                let mut value: Value = serde_json::from_str(&data)?;
                if value.get("status").and_then(Value::as_str) != Some(expected_status.as_str()) {
                    return Err(StorageError::Constraint {
                        code: "WB2009",
                        message: format!(
                            "{object_type} {id} 状态不是 {expected_status}，不能变更为 {next_status}"
                        ),
                    });
                }
                if let Some(status) = value.as_object_mut() {
                    status.insert("status".into(), json!(next_status));
                }
                let data = serde_json::to_string(&value)?;
                let updated = tx
                    .execute(
                        "UPDATE wb_ontology_object SET data=?1, version=version+1, \
                         update_time=CURRENT_TIMESTAMP WHERE id=?2 AND type=?3 AND is_delete=0 \
                         AND ((project_id = ?4) OR (project_id IS NULL AND ?4 IS NULL))",
                        params![
                            data.as_str(),
                            id.as_str(),
                            object_type.as_str(),
                            project_id.as_deref()
                        ],
                    )
                    .await?;
                if updated != 1 {
                    return Err(StorageError::NotFound(id.to_string()));
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn status_compare_and_set_is_atomic_across_connections() {
        let path = std::env::temp_dir().join(format!(
            "oaip_status_cas_{}_{}.db",
            std::process::id(),
            oaip_trace::now_ts()
        ));
        let _ = std::fs::remove_file(&path);
        let db_path = path.to_str().expect("临时路径应有效");
        let seed = LibSQLStorage::open(db_path).await.expect("数据库应可打开");
        seed.apply_world(WorldCommand::CreateObject {
            object_type: "Opportunity".into(),
            id: "opp-1".into(),
            data: json!({"status": "Decided"}),
            project_id: Some("project".into()),
        })
        .await
        .expect("种子机会应创建");
        let first = LibSQLStorage::open(db_path).await.expect("实例一应打开");
        let second = LibSQLStorage::open(db_path).await.expect("实例二应打开");

        let claim = |storage: LibSQLStorage| async move {
            storage
                .apply_world(WorldCommand::CompareSetStatus {
                    object_type: "Opportunity".into(),
                    id: "opp-1".into(),
                    project_id: Some("project".into()),
                    expected_status: "Decided".into(),
                    next_status: "Executed".into(),
                })
                .await
        };
        let (left, right) = tokio::join!(claim(first.clone()), claim(second.clone()));
        let ok_count = [left.is_ok(), right.is_ok()]
            .iter()
            .filter(|ok| **ok)
            .count();
        assert_eq!(ok_count, 1, "跨实例状态 CAS 只能成功一次");
        let record = seed.get("opp-1").await.expect("对象应可读取").unwrap();
        assert_eq!(record.data["status"], json!("Executed"));
        assert_eq!(record.version, 2);
        drop((first, second, seed));
        let _ = std::fs::remove_file(path);
    }
}
