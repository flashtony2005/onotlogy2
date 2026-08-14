use libsql::params;

use super::*;

pub struct RunRecord {
    pub id: String,
    pub agent_id: String,
    pub goal: String,
    pub status: String, // Running | Completed | Failed | Interrupted
    pub human_approval: bool,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
}

/// 一条待审批持久记录（与内存 `ApprovalStore` 同义，支持跨重启列表/审计）。
pub struct ApprovalRecord {
    pub id: String,
    pub run_id: String,
    pub agent_id: String,
    pub step_id: String,
    pub description: String,
    pub state: String, // Pending | Approved | Rejected | Expired
    pub created_at: String,
    pub updated_at: String,
    pub resolved_by: Option<String>,
    pub resolved_at: Option<String>,
}

impl LibSQLStorage {
    /// 插入一条 run（status='Running'，默认 human_approval=0）。热路径与内存映射并行写。
    pub async fn insert_run(
        &self,
        id: &str,
        agent_id: &str,
        goal: &str,
    ) -> Result<(), StorageError> {
        self.conn
            .execute(
                "INSERT INTO wb_run (id, agent_id, goal, status) VALUES (?1,?2,?3,'Running')",
                params![id, agent_id, goal],
            )
            .await?;
        Ok(())
    }

    /// 更新 run 终态（Completed/Failed/Interrupted）+ 完成时间（None → 不填）。
    pub async fn update_run_status(
        &self,
        id: &str,
        status: &str,
        finished_at: Option<&str>,
    ) -> Result<(), StorageError> {
        let finished: libsql::Value = match finished_at {
            Some(s) => libsql::Value::Text(s.to_string()),
            None => libsql::Value::Null,
        };
        self.conn
            .execute(
                "UPDATE wb_run SET status=?1, finished_at=?2, updated_at=CURRENT_TIMESTAMP WHERE id=?3",
                libsql::params![status, finished, id],
            )
            .await?;
        Ok(())
    }

    /// 列出全部 run（恢复重建内存映射用）。
    pub async fn list_runs(&self) -> Result<Vec<RunRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, agent_id, goal, status, human_approval, created_at, updated_at, finished_at \
                 FROM wb_run ORDER BY created_at ASC LIMIT 1000",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(row_to_run(&r)?);
        }
        Ok(out)
    }

    /// 按 id 取单条 run（timeline DB 回退用）。
    pub async fn get_run(&self, id: &str) -> Result<Option<RunRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, agent_id, goal, status, human_approval, created_at, updated_at, finished_at \
                 FROM wb_run WHERE id = ?1",
                params![id],
            )
            .await?;
        match rows.next().await? {
            Some(r) => Ok(Some(row_to_run(&r)?)),
            None => Ok(None),
        }
    }

    /// 插入一条待审批（status='Pending'）。
    pub async fn insert_approval(
        &self,
        req_id: &str,
        run_id: &str,
        agent_id: &str,
        step_id: &str,
        description: &str,
    ) -> Result<(), StorageError> {
        self.conn
            .execute(
                "INSERT INTO wb_approval (id, run_id, agent_id, step_id, description, state) \
                 VALUES (?1,?2,?3,?4,?5,'Pending')",
                params![req_id, run_id, agent_id, step_id, description],
            )
            .await?;
        Ok(())
    }

    /// 更新待审批裁决状态（resolved_by/resolved_at 为 None 时不填）。
    pub async fn update_approval_state(
        &self,
        id: &str,
        state: &str,
        resolved_by: Option<&str>,
        resolved_at: Option<&str>,
    ) -> Result<(), StorageError> {
        let by: libsql::Value = match resolved_by {
            Some(s) => libsql::Value::Text(s.to_string()),
            None => libsql::Value::Null,
        };
        let at: libsql::Value = match resolved_at {
            Some(s) => libsql::Value::Text(s.to_string()),
            None => libsql::Value::Null,
        };
        self.conn
            .execute(
                "UPDATE wb_approval SET state=?1, resolved_by=?2, resolved_at=?3, updated_at=CURRENT_TIMESTAMP WHERE id=?4",
                libsql::params![state, by, at, id],
            )
            .await?;
        Ok(())
    }

    /// 列出某 run 的全部待审批（跨重启列表/审计）。
    pub async fn list_approvals_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<ApprovalRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, run_id, agent_id, step_id, description, state, created_at, updated_at, resolved_by, resolved_at \
                 FROM wb_approval WHERE run_id = ?1 ORDER BY created_at ASC",
                params![run_id],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(row_to_approval(&r)?);
        }
        Ok(out)
    }

    /// 按状态列出待审批（恢复时取 Pending）。
    pub async fn list_approvals_by_state(
        &self,
        state: &str,
    ) -> Result<Vec<ApprovalRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, run_id, agent_id, step_id, description, state, created_at, updated_at, resolved_by, resolved_at \
                 FROM wb_approval WHERE state = ?1 ORDER BY created_at ASC",
                params![state],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(r) = rows.next().await? {
            out.push(row_to_approval(&r)?);
        }
        Ok(out)
    }

    /// 重启恢复（简化分支）：所有 Running → Interrupted，所有 Pending → Expired；
    /// 返回全部 run 供重建内存映射。无"活 run"可恢复，故不做"run 存活→Pending 保留"死分支。
    pub async fn recover_persisted_state(&self) -> Result<Vec<RunRecord>, StorageError> {
        self.conn
            .execute(
                "UPDATE wb_run SET status='Interrupted', updated_at=CURRENT_TIMESTAMP WHERE status='Running'",
                params![],
            )
            .await?;
        self.conn
            .execute(
                "UPDATE wb_approval SET state='Expired', updated_at=CURRENT_TIMESTAMP WHERE state='Pending'",
                params![],
            )
            .await?;
        self.list_runs().await
    }
}

fn row_to_run(row: &libsql::Row) -> Result<RunRecord, StorageError> {
    let finished_at: Option<String> = row.get(7)?;
    Ok(RunRecord {
        id: row.get(0)?,
        agent_id: row.get(1)?,
        goal: row.get(2)?,
        status: row.get(3)?,
        human_approval: row.get::<i64>(4)? != 0,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        finished_at,
    })
}

fn row_to_approval(row: &libsql::Row) -> Result<ApprovalRecord, StorageError> {
    let resolved_by: Option<String> = row.get(8)?;
    let resolved_at: Option<String> = row.get(9)?;
    Ok(ApprovalRecord {
        id: row.get(0)?,
        run_id: row.get(1)?,
        agent_id: row.get(2)?,
        step_id: row.get(3)?,
        description: row.get(4)?,
        state: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        resolved_by,
        resolved_at,
    })
}
