use libsql::params;

use super::*;

impl LibSQLStorage {
    // ---- Evolution Engine（BL-003 · REQ-017）：Experience → Knowledge，不修改 Policy ----

    /// 写入/合并一条 Pattern（按 label 幂等 upsert；run_ids 合并去重，沉淀跨 run 证据）。
    pub async fn save_pattern(
        &self,
        pattern: &crate::evolution::Pattern,
        run_ids: &[String],
    ) -> Result<(), StorageError> {
        // 读取已有 run_ids 以合并（证据跨 run 累加）
        let existing: Vec<String> = match self.read_pattern_run_ids(&pattern.label).await? {
            Some(s) => crate::evolution::parse_run_ids(&s),
            None => Vec::new(),
        };
        let merged = crate::evolution::merge_run_ids(existing, run_ids);
        let run_ids_s = crate::evolution::encode_run_ids(&merged);
        self.conn
            .execute(
                "INSERT INTO wb_pattern (label, action, support, confidence, run_ids, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP) \
                 ON CONFLICT(label) DO UPDATE SET \
                   support = ?3, confidence = ?4, run_ids = ?5, updated_at = CURRENT_TIMESTAMP",
                params![
                    pattern.label.as_str(),
                    pattern.action.as_str(),
                    pattern.support,
                    pattern.confidence,
                    run_ids_s
                ],
            )
            .await?;
        Ok(())
    }

    async fn read_pattern_run_ids(&self, label: &str) -> Result<Option<String>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT run_ids FROM wb_pattern WHERE label = ?1",
                params![label],
            )
            .await?;
        match rows.next().await? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    async fn read_proposal_run_ids(&self, id: &str) -> Result<Option<String>, StorageError> {
        let mut rows = self
            .conn
            .query("SELECT run_ids FROM wb_proposal WHERE id = ?1", params![id])
            .await?;
        match rows.next().await? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    async fn read_learned_run_ids(&self, id: &str) -> Result<Option<String>, StorageError> {
        let mut rows = self
            .conn
            .query("SELECT run_ids FROM wb_learned WHERE id = ?1", params![id])
            .await?;
        match rows.next().await? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// 列出全部 Pattern（按 support 降序）。
    pub async fn list_patterns(&self) -> Result<Vec<crate::evolution::Pattern>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT label, action, support, confidence, run_ids FROM wb_pattern \
                 ORDER BY support DESC, updated_at DESC LIMIT 1000",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_pattern(&row)?);
        }
        Ok(out)
    }

    /// 写入一条 Proposal（按 id 幂等 upsert）。
    pub async fn save_proposal(
        &self,
        proposal: &crate::evolution::Proposal,
    ) -> Result<(), StorageError> {
        let status = match proposal.status {
            crate::evolution::ProposalStatus::Proposed => "Proposed",
            crate::evolution::ProposalStatus::Validated => "Validated",
            crate::evolution::ProposalStatus::Rejected => "Rejected",
        };
        let kind = match proposal.kind {
            crate::evolution::ProposalKind::ActionPattern => "ActionPattern",
        };
        // 合并已有 run_ids（证据跨 run 累加）
        let existing = self
            .read_proposal_run_ids(&proposal.id)
            .await?
            .map(|s| crate::evolution::parse_run_ids(&s))
            .unwrap_or_default();
        let merged = crate::evolution::merge_run_ids(existing, &proposal.run_ids);
        let run_ids_s = crate::evolution::encode_run_ids(&merged);
        self.conn
            .execute(
                "INSERT INTO wb_proposal (id, kind, target, confidence, status, pattern_label, run_ids, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP) \
                 ON CONFLICT(id) DO UPDATE SET \
                   confidence = ?4, status = ?5, run_ids = ?7, updated_at = CURRENT_TIMESTAMP",
                params![
                    proposal.id.as_str(),
                    kind,
                    proposal.target.as_str(),
                    proposal.confidence,
                    status,
                    proposal.pattern_label.as_str(),
                    run_ids_s
                ],
            )
            .await?;
        Ok(())
    }

    /// 列出全部 Proposal（按置信度降序）。
    pub async fn list_proposals(&self) -> Result<Vec<crate::evolution::Proposal>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, kind, target, confidence, status, pattern_label, run_ids FROM wb_proposal \
                 ORDER BY confidence DESC, updated_at DESC LIMIT 1000",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_proposal(&row)?);
        }
        Ok(out)
    }

    /// 写入一条 LearnedKnowledge（按 id 幂等 upsert）。仅在 Proposal Validated 后调用。
    pub async fn save_learned(
        &self,
        learned: &crate::evolution::LearnedKnowledge,
    ) -> Result<(), StorageError> {
        let kind = match learned.kind {
            crate::evolution::ProposalKind::ActionPattern => "ActionPattern",
        };
        // 合并已有 run_ids（证据跨 run 累加）
        let existing = self
            .read_learned_run_ids(&learned.id)
            .await?
            .map(|s| crate::evolution::parse_run_ids(&s))
            .unwrap_or_default();
        let merged = crate::evolution::merge_run_ids(existing, &learned.run_ids);
        let run_ids_s = crate::evolution::encode_run_ids(&merged);
        self.conn
            .execute(
                "INSERT INTO wb_learned (id, kind, action, confidence, proposal_id, run_ids, auto_approve_enabled, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CURRENT_TIMESTAMP) \
                 ON CONFLICT(id) DO UPDATE SET \
                   confidence = ?4, proposal_id = ?5, run_ids = ?6, auto_approve_enabled = ?7",
                params![
                    learned.id.as_str(),
                    kind,
                    learned.action.as_str(),
                    learned.confidence,
                    learned.proposal_id.as_str(),
                    run_ids_s,
                    learned.auto_approve_enabled as i64,
                ],
            )
            .await?;
        Ok(())
    }

    /// 列出全部 LearnedKnowledge（按置信度降序）。
    pub async fn list_learned(
        &self,
    ) -> Result<Vec<crate::evolution::LearnedKnowledge>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, kind, action, confidence, proposal_id, run_ids, auto_approve_enabled, created_at FROM wb_learned \
                 ORDER BY confidence DESC, created_at DESC LIMIT 1000",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(row_to_learned(&row)?);
        }
        Ok(out)
    }

    /// 撤销/删除一条 LearnedKnowledge（幂等：不存在也返回 Ok）。
    pub async fn delete_learned(&self, id: &str) -> Result<(), StorageError> {
        self.conn
            .execute("DELETE FROM wb_learned WHERE id = ?1", params![id])
            .await?;
        Ok(())
    }

    /// REQ-018：翻转一条 LearnedKnowledge 的自动放行资格（可撤销承诺，幂等）。
    /// 置 false 后该 action 即使满足其余 4 道闸门也回退 Human Approval（决不静默放行）。
    pub async fn set_learned_auto_approve(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<(), StorageError> {
        self.conn
            .execute(
                "UPDATE wb_learned SET auto_approve_enabled = ?2 WHERE id = ?1",
                params![id, enabled as i64],
            )
            .await?;
        Ok(())
    }

    /// REQ-018：写入一条自动放行审计（可追溯至 LearnedKnowledge + run）。
    pub async fn record_approval_audit(
        &self,
        run_id: &str,
        action: &str,
        learned_id: &str,
        decision: &str,
        reason: &str,
    ) -> Result<(), StorageError> {
        let id = format!(
            "audit_{}_{}",
            now_ts(),
            TRACE_SEQ_AUDIT.fetch_add(1, Ordering::SeqCst)
        );
        self.conn
            .execute(
                "INSERT INTO wb_approval_audit (id, run_id, action, learned_id, decision, reason) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, run_id, action, learned_id, decision, reason],
            )
            .await?;
        Ok(())
    }

    /// REQ-018：列出自动放行审计（按 id 过滤单条 LearnedKnowledge，或全量）。
    pub async fn list_approval_audit(
        &self,
        learned_id: Option<&str>,
    ) -> Result<Vec<ApprovalAudit>, StorageError> {
        let mut rows = match learned_id {
            Some(lid) => {
                self.conn
                    .query(
                        "SELECT id, run_id, action, learned_id, decision, reason, created_at \
                         FROM wb_approval_audit WHERE learned_id = ?1 \
                         ORDER BY created_at DESC, id DESC",
                        params![lid],
                    )
                    .await?
            }
            None => {
                self.conn
                    .query(
                        "SELECT id, run_id, action, learned_id, decision, reason, created_at \
                         FROM wb_approval_audit ORDER BY created_at DESC, id DESC",
                        params![],
                    )
                    .await?
            }
        };
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(ApprovalAudit {
                id: row.get(0)?,
                run_id: row.get(1)?,
                action: row.get(2)?,
                learned_id: row.get(3)?,
                decision: row.get(4)?,
                reason: row.get(5)?,
                created_at: row.get(6)?,
            });
        }
        Ok(out)
    }

    /// REQ-018：统计某动作的历史 ActionFailed 事件数（强判定「无失败」闸门）。
    /// ActionFailed 事件的 `input` 存 `{"action":"<限定键>"}`，据此精确匹配（避免误伤同名）。
    /// M6：键值经 LIKE 通配符转义，`%`/`_` 不会被当通配符放大。
    pub async fn count_action_failed(&self, action: &str) -> Result<usize, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT COUNT(*) FROM wb_trace_event \
                 WHERE event_type = 'ActionFailed' AND input LIKE ?1 ESCAPE '\\'",
                params![format!("%\"action\":\"{}\"%", escape_like(action))],
            )
            .await?;
        let cnt: i64 = match rows.next().await? {
            Some(row) => row.get(0)?,
            None => 0,
        };
        Ok(cnt.max(0) as usize)
    }
}
