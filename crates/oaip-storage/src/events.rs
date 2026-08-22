use libsql::{Transaction, params};
use sha2::{Digest, Sha256};

use super::*;

const AUDIT_HASH_SEED: &[u8] = b"oaip-audit-v1";

fn digest_hex(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    hex::encode(digest.finalize())
}

fn hash0() -> String {
    digest_hex(AUDIT_HASH_SEED)
}

fn canonical_event(ev: &TraceEvent) -> Result<Vec<u8>, StorageError> {
    Ok(serde_json::to_vec(ev)?)
}

fn next_hash(previous_hash: &str, ev: &TraceEvent) -> Result<String, StorageError> {
    let previous = hex::decode(previous_hash).map_err(|e| StorageError::Constraint {
        code: "WB2103",
        message: format!("审计链 previous_hash 非法: {e}"),
    })?;
    let event = canonical_event(ev)?;
    let mut bytes = Vec::with_capacity(previous.len() + event.len());
    bytes.extend_from_slice(&previous);
    bytes.extend_from_slice(&event);
    Ok(digest_hex(&bytes))
}

#[derive(Debug, Clone, serde::Serialize)]
struct ExportEvent {
    sequence: i64,
    event: TraceEvent,
    previous_hash: String,
    hash: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct AuditExport {
    format: &'static str,
    hash_algorithm: &'static str,
    seed: &'static str,
    event_count: usize,
    head_hash: String,
    events: Vec<ExportEvent>,
}

/// 在调用方事务内追加 trace 事件及其审计链节点。
///
/// 世界命令也通过此入口写事件，确保事件、领域状态和哈希链处于同一事务，
/// 不允许出现“事件已落库但链缺节点”的不可审计状态。热路径只验证链头，
/// 不重新扫描历史事件；完整扫描由 `validate_audit_chain_deep` 负责。
pub(crate) async fn append_trace_tx(tx: &Transaction, ev: &TraceEvent) -> Result<(), StorageError> {
    if ev.object_refs.is_empty() {
        return Err(StorageError::Constraint {
            code: "WB2006",
            message: "TraceEvent 缺 object_refs，禁止落库".into(),
        });
    }
    let (sequence, previous) = audit_head_tx(tx).await?;
    let hash = next_hash(&previous, ev)?;
    let obj_refs = serde_json::to_string(&ev.object_refs)?;
    let input = ev.input.as_ref().map(serde_json::to_string).transpose()?;
    let output = ev.output.as_ref().map(serde_json::to_string).transpose()?;
    tx.execute(
        "INSERT INTO wb_trace_event \
         (id, kind, event_type, object_refs, project_id, run_id, agent_id, reasoning, input, output, confidence, timestamp) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![
            ev.id.clone(),
            ev.kind.as_str(),
            ev.event_type.clone(),
            obj_refs,
            ev.project_id.clone(),
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
    tx.execute(
        "INSERT INTO wb_audit_chain (sequence, event_id, previous_hash, hash) VALUES (?1,?2,?3,?4)",
        params![sequence, ev.id.clone(), previous, hash.clone()],
    )
    .await?;
    write_audit_head_meta_tx(tx, sequence, &ev.id, &hash).await?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AuditHeadSnapshot {
    sequence: i64,
    event_id: String,
    hash: String,
}

const AUDIT_HEAD_META_KEY: &str = "audit_head";

async fn read_audit_head_meta_tx(
    tx: &Transaction,
) -> Result<Option<AuditHeadSnapshot>, StorageError> {
    let mut rows = tx
        .query(
            "SELECT value FROM wb_meta WHERE key=?1",
            params![AUDIT_HEAD_META_KEY],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(None);
    };
    let value: Option<String> = row.get(0)?;
    let value = value.ok_or_else(|| StorageError::Constraint {
        code: "WB2103",
        message: "审计链头元数据为空".into(),
    })?;
    serde_json::from_str(&value)
        .map(Some)
        .map_err(|error| StorageError::Constraint {
            code: "WB2103",
            message: format!("审计链头元数据非法: {error}"),
        })
}

async fn write_audit_head_meta_tx(
    tx: &Transaction,
    sequence: i64,
    event_id: &str,
    hash: &str,
) -> Result<(), StorageError> {
    let value = serde_json::to_string(&AuditHeadSnapshot {
        sequence,
        event_id: event_id.into(),
        hash: hash.into(),
    })?;
    tx.execute(
        "INSERT OR REPLACE INTO wb_meta (key, value) VALUES (?1, ?2)",
        params![AUDIT_HEAD_META_KEY, value],
    )
    .await?;
    Ok(())
}

/// 热路径链尾校验：只读取持久化链头及其相邻节点，不扫描历史事件。
/// 完整事件数量与 hash 链校验由 `validate_audit_chain_deep` 负责。
async fn audit_head_tx(tx: &Transaction) -> Result<(i64, String), StorageError> {
    let metadata = read_audit_head_meta_tx(tx).await?;
    let mut rows = tx
        .query(
            "SELECT sequence, event_id, previous_hash, hash \
             FROM wb_audit_chain ORDER BY sequence DESC LIMIT 1",
            params![],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        if let Some(head) = metadata
            && (head.sequence != 0 || !head.event_id.is_empty() || head.hash != hash0())
        {
            return Err(StorageError::Constraint {
                code: "WB2103",
                message: "空审计链的链头元数据非法".into(),
            });
        }
        return Ok((1, hash0()));
    };

    let sequence: i64 = row.get(0)?;
    let event_id: String = row.get(1)?;
    let previous_hash: String = row.get(2)?;
    let head_hash: String = row.get(3)?;
    if sequence <= 0 {
        return Err(StorageError::Constraint {
            code: "WB2103",
            message: format!("审计链头 sequence 非法: sequence={sequence}"),
        });
    }
    if let Some(head) = metadata
        && (head.sequence != sequence || head.event_id != event_id || head.hash != head_hash)
    {
        return Err(StorageError::Constraint {
            code: "WB2103",
            message: format!("审计链头元数据不匹配于 sequence={sequence}"),
        });
    }
    let expected_previous = if sequence == 1 {
        hash0()
    } else {
        let mut previous_rows = tx
            .query(
                "SELECT hash FROM wb_audit_chain WHERE sequence = ?1",
                params![sequence - 1],
            )
            .await?;
        previous_rows
            .next()
            .await?
            .ok_or_else(|| StorageError::Constraint {
                code: "WB2103",
                message: format!("审计链缺少前置节点: sequence={}", sequence - 1),
            })?
            .get::<String>(0)?
    };
    if previous_hash != expected_previous {
        return Err(StorageError::Constraint {
            code: "WB2103",
            message: format!("审计链头 previous_hash 不匹配于 sequence={sequence}"),
        });
    }
    let mut event_rows = tx
        .query(
            "SELECT id, kind, event_type, object_refs, project_id, run_id, agent_id, reasoning, input, output, confidence, timestamp \
             FROM wb_trace_event WHERE id = ?1",
            params![event_id.clone()],
        )
        .await?;
    let event = event_rows
        .next()
        .await?
        .ok_or_else(|| StorageError::Constraint {
            code: "WB2103",
            message: format!("审计链头引用不存在的事件: {event_id}"),
        })
        .and_then(|row| super::row_to_trace(&row))?;
    let expected_hash = next_hash(&previous_hash, &event)?;
    if head_hash != expected_hash {
        return Err(StorageError::Constraint {
            code: "WB2103",
            message: format!("审计 hash 不匹配于 sequence={sequence}"),
        });
    }
    Ok((
        sequence
            .checked_add(1)
            .ok_or_else(|| StorageError::Constraint {
                code: "WB2103",
                message: "审计链 sequence 溢出".into(),
            })?,
        head_hash,
    ))
}

pub(crate) fn is_audit_chain_error(error: &StorageError) -> bool {
    matches!(error, StorageError::Constraint { code: "WB2103", .. })
}

impl LibSQLStorage {
    async fn sync_audit_head(&self) -> Result<(), StorageError> {
        let tx = self.conn.transaction().await?;
        let mut rows = tx
            .query(
                "SELECT sequence, event_id, hash \
                 FROM wb_audit_chain ORDER BY sequence DESC LIMIT 1",
                params![],
            )
            .await?;
        if let Some(row) = rows.next().await? {
            let sequence: i64 = row.get(0)?;
            let event_id: String = row.get(1)?;
            let hash: String = row.get(2)?;
            write_audit_head_meta_tx(&tx, sequence, &event_id, &hash).await?;
        } else {
            write_audit_head_meta_tx(&tx, 0, "", &hash0()).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// 确保事件与哈希链一一对应。全新升级的旧库只允许「整条链为空」时确定性补建；
    /// 部分链、顺序错乱或内容不一致直接拒绝，避免静默掩盖篡改。
    pub async fn ensure_audit_chain(&self) -> Result<(), StorageError> {
        let result = async {
            let events = self.list_all_events().await?;
            let chain = self.list_audit_chain().await?;
            if chain.is_empty() && !events.is_empty() {
                let tx = self.conn.transaction().await?;
                let mut previous = hash0();
                for (idx, event) in events.iter().enumerate() {
                    let sequence = i64::try_from(idx + 1).map_err(|_| StorageError::Constraint {
                        code: "WB2103",
                        message: "审计事件数量超出 sequence 范围".into(),
                    })?;
                    let hash = next_hash(&previous, event)?;
                    tx.execute(
                        "INSERT INTO wb_audit_chain (sequence, event_id, previous_hash, hash) VALUES (?1,?2,?3,?4)",
                        params![sequence, event.id.clone(), previous.clone(), hash.clone()],
                    )
                    .await?;
                    previous = hash;
                }
                tx.commit().await?;
            }
            self.validate_audit_chain_deep().await?;
            self.sync_audit_head().await
        }
        .await;
        match result {
            Ok(()) => {
                self.mark_audit_chain_valid();
                Ok(())
            }
            Err(error) => {
                self.mark_audit_chain_invalid();
                Err(error)
            }
        }
    }

    /// 追加事件与链节点必须在同一事务中提交；热路径只校验链头。
    pub async fn append_trace(&self, ev: &TraceEvent) -> Result<(), StorageError> {
        #[cfg(test)]
        crate::io_fault_if_requested("trace.before_insert")?;
        let result = async {
            let tx = self.conn.transaction().await?;
            append_trace_tx(&tx, ev).await?;
            tx.commit().await?;
            Ok(())
        }
        .await;
        match result {
            Ok(()) => {
                self.mark_audit_chain_valid();
                Ok(())
            }
            Err(error) => {
                if is_audit_chain_error(&error) {
                    self.mark_audit_chain_invalid();
                }
                Err(error)
            }
        }
    }

    /// 原子轮换认证会话并记录审计事件。
    ///
    /// 会话撤销与 `auth.key_rotated` 事件必须位于同一事务中：审计事件
    /// 校验或提交失败时，旧会话仍保持有效，避免出现“实际轮换但无审计记录”。
    pub async fn rotate_auth_key(&self, event: &TraceEvent) -> Result<u64, StorageError> {
        let result = async {
            let tx = self.conn.transaction().await?;
            let revoked = tx.execute("DELETE FROM wb_session", params![]).await?;
            let mut event = event.clone();
            event.input = Some(serde_json::json!({ "revokedSessions": revoked }));
            append_trace_tx(&tx, &event).await?;
            tx.commit().await?;
            Ok(revoked)
        }
        .await;
        match result {
            Ok(revoked) => {
                self.mark_audit_chain_valid();
                Ok(revoked)
            }
            Err(error) => {
                if is_audit_chain_error(&error) {
                    self.mark_audit_chain_invalid();
                }
                Err(error)
            }
        }
    }

    /// 返回持久化 trace 事件总数，不加载事件正文。
    pub async fn count_trace_events(&self) -> Result<i64, StorageError> {
        let mut rows = self
            .conn
            .query("SELECT COUNT(*) FROM wb_trace_event", params![])
            .await?;
        match rows.next().await? {
            Some(row) => Ok(row.get(0)?),
            None => Ok(0),
        }
    }

    /// 返回完整、稳定排序的事件集（导出与链校验专用，不受普通查询的 1000 条窗口影响）。
    pub async fn list_all_events(&self) -> Result<Vec<TraceEvent>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, kind, event_type, object_refs, project_id, run_id, agent_id, reasoning, input, output, confidence, timestamp \
                 FROM wb_trace_event ORDER BY timestamp ASC, id ASC",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_trace(&row)?);
        }
        Ok(out)
    }

    pub async fn list_audit_chain(&self) -> Result<Vec<AuditChainRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT sequence, event_id, previous_hash, hash FROM wb_audit_chain ORDER BY sequence ASC",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(AuditChainRecord {
                sequence: row.get(0)?,
                event_id: row.get(1)?,
                previous_hash: row.get(2)?,
                hash: row.get(3)?,
            });
        }
        Ok(out)
    }

    /// 校验完整性链；任何缺失、重排、事件替换或 hash 不匹配都失败。
    pub async fn validate_audit_chain(&self) -> Result<(), StorageError> {
        let result = self.validate_audit_chain_deep().await;
        match result {
            Ok(()) => {
                self.mark_audit_chain_valid();
                Ok(())
            }
            Err(error) => {
                self.mark_audit_chain_invalid();
                Err(error)
            }
        }
    }

    /// 冷路径完整审计校验。启动、显式审计检查和导出流程使用；不得放入请求热路径。
    pub async fn validate_audit_chain_deep(&self) -> Result<(), StorageError> {
        let mut count_rows = self
            .conn
            .query("SELECT COUNT(*) FROM wb_trace_event", params![])
            .await?;
        let event_count = count_rows
            .next()
            .await?
            .ok_or_else(|| StorageError::Constraint {
                code: "WB2103",
                message: "无法读取审计事件数量".into(),
            })?
            .get::<i64>(0)?;
        let chain = self.list_audit_chain().await?;
        let chain_count = i64::try_from(chain.len()).map_err(|_| StorageError::Constraint {
            code: "WB2103",
            message: "审计链节点数量超出计数范围".into(),
        })?;
        if event_count != chain_count {
            return Err(StorageError::Constraint {
                code: "WB2103",
                message: format!(
                    "审计链长度不一致: events={event_count}, chain={}",
                    chain.len()
                ),
            });
        }
        let mut previous = hash0();
        for (idx, node) in chain.iter().enumerate() {
            let expected_sequence =
                i64::try_from(idx + 1).map_err(|_| StorageError::Constraint {
                    code: "WB2103",
                    message: "审计事件数量超出 sequence 范围".into(),
                })?;
            let Some(event) = self.get_trace(&node.event_id).await? else {
                return Err(StorageError::Constraint {
                    code: "WB2103",
                    message: format!("审计链引用不存在的事件: {}", node.event_id),
                });
            };
            if node.sequence != expected_sequence || node.previous_hash != previous {
                return Err(StorageError::Constraint {
                    code: "WB2103",
                    message: format!("审计链断裂于 sequence={expected_sequence}"),
                });
            }
            let expected_hash = next_hash(&previous, &event)?;
            if node.hash != expected_hash {
                return Err(StorageError::Constraint {
                    code: "WB2103",
                    message: format!("审计 hash 不匹配于 sequence={expected_sequence}"),
                });
            }
            previous = node.hash.clone();
        }
        Ok(())
    }

    /// 生成稳定 JSON 导出。
    pub async fn audit_export_json(&self) -> Result<String, StorageError> {
        self.audit_export_json_with_event(None).await
    }

    /// 生成包含“待追加事件”的稳定 JSON 导出，但不修改数据库。
    ///
    /// CLI 先把完整内容写入新文件，再追加 `audit.exported` 事件；这样目标
    /// 文件创建/写入失败时不会推进审计链，而成功导出的内容仍明确包含自身事件。
    pub async fn audit_export_json_with_event(
        &self,
        extra: Option<&TraceEvent>,
    ) -> Result<String, StorageError> {
        self.validate_audit_chain().await?;
        if let Some(event) = extra
            && event.object_refs.is_empty()
        {
            return Err(StorageError::Constraint {
                code: "WB2006",
                message: "TraceEvent 缺 object_refs，禁止导出".into(),
            });
        }
        let chain = self.list_audit_chain().await?;
        let mut head_hash = chain.last().map(|n| n.hash.clone()).unwrap_or_else(hash0);
        let mut records = Vec::with_capacity(chain.len() + usize::from(extra.is_some()));
        for node in chain {
            let event =
                self.get_trace(&node.event_id)
                    .await?
                    .ok_or_else(|| StorageError::Constraint {
                        code: "WB2103",
                        message: format!("审计链引用不存在的事件: {}", node.event_id),
                    })?;
            records.push(ExportEvent {
                sequence: node.sequence,
                event,
                previous_hash: node.previous_hash,
                hash: node.hash,
            });
        }
        if let Some(event) = extra {
            let sequence = records
                .last()
                .map(|record| record.sequence)
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| StorageError::Constraint {
                    code: "WB2103",
                    message: "审计事件数量超出 sequence 范围".into(),
                })?;
            let previous_hash = head_hash.clone();
            let hash = next_hash(&previous_hash, event)?;
            records.push(ExportEvent {
                sequence,
                event: event.clone(),
                previous_hash,
                hash: hash.clone(),
            });
            head_hash = hash;
        }
        Ok(serde_json::to_string_pretty(&AuditExport {
            format: "oaip.audit.export.v1",
            hash_algorithm: "SHA-256",
            seed: "oaip-audit-v1",
            event_count: records.len(),
            head_hash,
            events: records,
        })?)
    }

    /// 按条件查询 Trace 事件（认知/世界同源）。
    /// [强制] after_id 游标用于 WebSocket 断线重连补齐：返回该事件之后的全部匹配事件（按 timestamp,id 升序）。
    pub async fn list_events(&self, q: TraceQuery) -> Result<Vec<TraceEvent>, StorageError> {
        let mut sql = String::from(
            "SELECT id, kind, event_type, object_refs, project_id, run_id, agent_id, reasoning, input, output, confidence, timestamp \
             FROM wb_trace_event WHERE 1=1",
        );
        let mut params: Vec<libsql::Value> = Vec::new();

        if let Some(v) = &q.project_id {
            sql.push_str(" AND project_id = ?");
            params.push(libsql::Value::Text(v.clone()));
        }
        if let Some(v) = &q.run_id {
            sql.push_str(" AND run_id = ?");
            params.push(libsql::Value::Text(v.clone()));
        }
        if let Some(v) = &q.agent_id {
            sql.push_str(" AND agent_id = ?");
            params.push(libsql::Value::Text(v.clone()));
        }
        if let Some(v) = &q.event_type {
            sql.push_str(" AND event_type = ?");
            params.push(libsql::Value::Text(v.clone()));
        }
        if let Some(v) = &q.object_ref {
            // object_refs 存 JSON 数组文本，按 `"id"` 子串匹配（反查经 wb_ontology_object.id）。
            // M6：输入经 LIKE 通配符转义，防止 %/_ 被当通配符放大匹配。
            sql.push_str(" AND object_refs LIKE ? ESCAPE '\\'");
            params.push(libsql::Value::Text(format!("%\"{}\"%", escape_like(v))));
        }
        if let Some(after) = &q.after_id {
            // 解析游标事件 (timestamp, id)，返回其后事件（断线重连补齐）。
            let mut marker = self
                .conn
                .query(
                    "SELECT timestamp, id FROM wb_trace_event WHERE id = ?",
                    libsql::params![after.clone()],
                )
                .await?;
            if let Some(row) = marker.next().await? {
                let ts: String = row.get(0)?;
                let id: String = row.get(1)?;
                sql.push_str(" AND (timestamp > ? OR (timestamp = ? AND id > ?))");
                params.push(libsql::Value::Text(ts.clone()));
                params.push(libsql::Value::Text(ts));
                params.push(libsql::Value::Text(id));
            }
        }
        if q.newest_first {
            sql.push_str(" ORDER BY timestamp DESC, id DESC");
        } else {
            sql.push_str(" ORDER BY timestamp ASC, id ASC");
        }
        sql.push_str(&format!(" LIMIT {}", q.limit.clamp(1, 1000)));

        let mut rows = self
            .conn
            .query(&sql, libsql::params_from_iter(params))
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_trace(&row)?);
        }
        Ok(out)
    }

    /// 按 ID 取单条 Trace 事件（供 `GET /trace/{id}`，缺失返 None → WB2002）。
    pub async fn get_trace(&self, id: &str) -> Result<Option<TraceEvent>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, kind, event_type, object_refs, project_id, run_id, agent_id, reasoning, input, output, confidence, timestamp \
                 FROM wb_trace_event WHERE id = ?",
                params![id],
            )
            .await?;
        if let Some(row) = rows.next().await? {
            Ok(Some(row_to_trace(&row)?))
        } else {
            Ok(None)
        }
    }

    /// 列出 world 事件（按时间升序），供 `oaip event list` 与 TC-008 校验。
    pub async fn list_world_events(&self) -> Result<Vec<TraceEvent>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, kind, event_type, object_refs, project_id, run_id, agent_id, reasoning, input, output, confidence, timestamp \
                 FROM wb_trace_event WHERE kind='world' ORDER BY timestamp ASC LIMIT 1000",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_trace(&row)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod audit_chain_tests {
    use super::*;
    use oaip_trace::TraceKind;
    use serde_json::json;

    fn event(id: &str) -> TraceEvent {
        TraceEvent {
            id: id.into(),
            kind: TraceKind::Cognitive,
            event_type: "audit.test".into(),
            object_refs: vec!["audit-test".into()],
            project_id: None,
            run_id: None,
            agent_id: Some("test".into()),
            reasoning: None,
            input: Some(json!({ "id": id })),
            output: None,
            confidence: None,
            timestamp: id.into(),
        }
    }

    #[tokio::test]
    async fn audit_head_metadata_tracks_latest_append() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        storage.append_trace(&event("event-1")).await.unwrap();
        storage.append_trace(&event("event-2")).await.unwrap();
        let mut rows = storage
            .conn
            .query(
                "SELECT value FROM wb_meta WHERE key=?1",
                params![AUDIT_HEAD_META_KEY],
            )
            .await
            .unwrap();
        let value: String = rows.next().await.unwrap().unwrap().get(0).unwrap();
        let head: AuditHeadSnapshot = serde_json::from_str(&value).unwrap();
        assert_eq!(head.sequence, 2);
        assert_eq!(head.event_id, "event-2");
        assert!(!head.hash.is_empty());
    }

    #[tokio::test]
    async fn tampered_event_fails_closed() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        storage.append_trace(&event("event-1")).await.unwrap();
        storage
            .conn
            .execute(
                "DROP TRIGGER trg_wb_trace_event_append_only_update",
                params![],
            )
            .await
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE wb_trace_event SET event_type='audit.tampered' WHERE id='event-1'",
                params![],
            )
            .await
            .unwrap();
        let err = storage
            .validate_audit_chain()
            .await
            .expect_err("事件内容篡改必须 fail-closed");
        assert!(err.to_string().contains("hash 不匹配"));
    }

    #[tokio::test]
    async fn sequence_gap_fails_closed() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        storage.append_trace(&event("event-1")).await.unwrap();
        storage.append_trace(&event("event-2")).await.unwrap();
        storage
            .conn
            .execute(
                "DROP TRIGGER trg_wb_audit_chain_append_only_update",
                params![],
            )
            .await
            .unwrap();
        storage
            .conn
            .execute(
                "UPDATE wb_audit_chain SET sequence=3 WHERE event_id='event-2'",
                params![],
            )
            .await
            .unwrap();
        let err = storage
            .validate_audit_chain()
            .await
            .expect_err("sequence 断裂必须 fail-closed");
        assert!(err.to_string().contains("断裂"));
    }

    #[tokio::test]
    async fn missing_chain_node_fails_closed() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        storage.append_trace(&event("event-1")).await.unwrap();
        storage
            .conn
            .execute(
                "DROP TRIGGER trg_wb_audit_chain_append_only_delete",
                params![],
            )
            .await
            .unwrap();
        storage
            .conn
            .execute("DELETE FROM wb_audit_chain WHERE sequence=1", params![])
            .await
            .unwrap();
        let err = storage
            .validate_audit_chain()
            .await
            .expect_err("缺失审计链节点必须 fail-closed");
        assert!(err.to_string().contains("长度不一致"));
    }
}
