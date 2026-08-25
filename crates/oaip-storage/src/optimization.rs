//! 业务优化运行历史：把一次目标导向优化的完整决策过程存为不可变审计记录。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use libsql::params;

use super::LibSQLStorage;
use super::StorageError;

/// 一条持久化优化运行；`payload` 是业务层的完整序列化结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BusinessOptimizationRunRecord {
    pub id: String,
    pub project_id: String,
    pub payload: Value,
    pub created_at: String,
}

impl LibSQLStorage {
    /// 追加一条优化运行历史。调用方必须在 payload 中生成 id 和时间戳。
    pub async fn save_business_optimization_run(
        &self,
        record: &BusinessOptimizationRunRecord,
    ) -> Result<(), StorageError> {
        let goal_metric = record.payload.get("goalMetric").and_then(Value::as_str);
        let stopped_as = record
            .payload
            .get("stoppedBecause")
            .and_then(Value::as_str)
            .ok_or_else(|| StorageError::Constraint {
                code: "WB2011",
                message: "优化运行缺少停止原因".into(),
            })?;
        self.conn
            .execute(
                "INSERT INTO wb_business_optimization_run \
             (id, project_id, goal_metric, stopped_as, payload, created_at) \
             VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    record.id.as_str(),
                    record.project_id.as_str(),
                    goal_metric,
                    stopped_as,
                    serde_json::to_string(&record.payload)?,
                    record.created_at.as_str()
                ],
            )
            .await?;
        Ok(())
    }

    /// 按项目返回最新优化历史；默认上限 50，最大 200。
    pub async fn list_business_optimization_runs(
        &self,
        project_id: &str,
        limit: u32,
    ) -> Result<Vec<BusinessOptimizationRunRecord>, StorageError> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, project_id, payload, created_at \
                 FROM wb_business_optimization_run \
                 WHERE project_id=?1 \
                 ORDER BY created_at DESC, id DESC LIMIT ?2",
                params![project_id, i64::from(limit.clamp(1, 200))],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(BusinessOptimizationRunRecord {
                id: row.get(0)?,
                project_id: row.get(1)?,
                payload: serde_json::from_str(&row.get::<String>(2)?)?,
                created_at: row.get(3)?,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn optimization_history_is_scoped_and_ordered() {
        let storage = LibSQLStorage::open(":memory:").await.unwrap();
        let save =
            |id: &'static str, project: &'static str, gap: f64| BusinessOptimizationRunRecord {
                id: id.into(),
                project_id: project.into(),
                payload: json!({
                    "id": id,
                    "projectId": project,
                    "finalGap": gap,
                    "stoppedBecause": "GoalReached"
                }),
                created_at: if id == "run-1" {
                    "2026-01-01T00:00:00Z".into()
                } else {
                    "2026-01-02T00:00:00Z".into()
                },
            };
        storage
            .save_business_optimization_run(&save("run-1", "project", 2.0))
            .await
            .unwrap();
        storage
            .save_business_optimization_run(&save("run-2", "project", 0.0))
            .await
            .unwrap();
        storage
            .save_business_optimization_run(&save("other", "other-project", 1.0))
            .await
            .unwrap();

        let history = storage
            .list_business_optimization_runs("project", 10)
            .await
            .unwrap();
        assert_eq!(
            history
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["run-2", "run-1"]
        );
        assert_eq!(history[0].payload["projectId"], json!("project"));
    }
}
