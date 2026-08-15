//! CognitiveStore —— 认知状态持久化（Phase 10 收口：Memory / Evolution / Rule /
//! Skill Registry / Session Log / Plugin 状态不再是内存态）。
//!
//! 单表 KV（`wbc_cognitive(key, value)`）+ JSON 序列化；支持本地文件与
//! Turso/libSQL server（URL 自动分类，token 取 TURSO_AUTH_TOKEN）。
//!
//! 与 Kernel 快照同一哲学：**显式 save / restore**，不搞隐式自动写。

use serde::Serialize;
use serde::de::DeserializeOwned;

/// 认知状态存储错误。
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("数据库错误: {0}")]
    Db(#[from] libsql::Error),
    #[error("序列化错误: {0}")]
    Json(#[from] serde_json::Error),
}

const DDL: &str = "
CREATE TABLE IF NOT EXISTS wbc_cognitive (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

/// 认知状态存储（libsql KV）。
#[derive(Clone)]
pub struct CognitiveStore {
    conn: libsql::Connection,
}

impl CognitiveStore {
    /// 打开（本地路径 / :memory: / Turso server URL 自动分类）。
    pub async fn open(path: &str) -> Result<Self, StoreError> {
        let p = path.to_ascii_lowercase();
        let remote = p.starts_with("libsql://")
            || p.starts_with("ws://")
            || p.starts_with("wss://")
            || p.starts_with("http://")
            || p.starts_with("https://");
        let db = if remote {
            let token = std::env::var("TURSO_AUTH_TOKEN").unwrap_or_default();
            libsql::Builder::new_remote(path.to_string(), token)
                .build()
                .await?
        } else {
            libsql::Builder::new_local(path).build().await?
        };
        let conn = db.connect()?;
        conn.execute_batch(DDL).await?;
        Ok(Self { conn })
    }

    /// 保存（UPSERT）。
    pub async fn save<T: Serialize>(&self, key: &str, value: &T) -> Result<(), StoreError> {
        let json = serde_json::to_string(value)?;
        self.conn
            .execute(
                "INSERT OR REPLACE INTO wbc_cognitive (key, value) VALUES (?1, ?2)",
                libsql::params![key, json],
            )
            .await?;
        Ok(())
    }

    /// 加载（不存在 → None）。
    pub async fn load<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, StoreError> {
        let mut rows = self
            .conn
            .query(
                "SELECT value FROM wbc_cognitive WHERE key = ?1",
                libsql::params![key],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Ok(None);
        };
        let json: String = row.get(0)?;
        Ok(Some(serde_json::from_str(&json)?))
    }

    /// 删除。
    pub async fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.conn
            .execute(
                "DELETE FROM wbc_cognitive WHERE key = ?1",
                libsql::params![key],
            )
            .await?;
        Ok(())
    }
}
