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
        #[cfg(test)]
        crate::crash_if_requested("run.after_insert");
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

    /// 返回持久化 run 总数，不加载业务字段。
    pub async fn count_runs(&self) -> Result<i64, StorageError> {
        let mut rows = self
            .conn
            .query("SELECT COUNT(*) FROM wb_run", params![])
            .await?;
        match rows.next().await? {
            Some(row) => Ok(row.get(0)?),
            None => Ok(0),
        }
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

    /// 重启恢复：所有残留 Running → Interrupted；返回全部 run 供重建内存映射。
    ///
    /// 审批不再通过已退役的 `wb_approval` 表恢复；活动审批由 Kernel/AppState
    /// 的显式裁决路径负责，避免恢复时误读旧存储入口。
    pub async fn recover_persisted_state(&self) -> Result<Vec<RunRecord>, StorageError> {
        self.conn
            .execute(
                "UPDATE wb_run SET status='Interrupted', updated_at=CURRENT_TIMESTAMP WHERE status='Running'",
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
