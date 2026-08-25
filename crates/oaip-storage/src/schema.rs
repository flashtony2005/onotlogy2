//! Schema 版本化迁移与运行时 schema 校验。
//!
//! `wb_meta.schema_version` 负责兼容已有数据库的启动判定；
//! `wb_schema_migration` 记录可审计的迁移历史。canonical schema 只在新库
//! 初始化或成功升级后用于补齐声明式对象，已是当前版本的数据库则先校验、
//! 不静默修复漂移。

use crate::StorageError;
use libsql::{Connection, params};
use oaip_abi::codegen;
use std::collections::BTreeMap;

/// 版本化迁移注册表：`(from_version, sql)`，按版本升序执行；每步成功后推进
/// `wb_meta.schema_version`。迁移在事务内执行（失败整体回滚，版本不推进，可安全重试）。
/// 新库由 `canonical_schema` 直接建到当前版本，不执行迁移。
pub(crate) const SCHEMA_VERSION: i64 = 16;

const MIGRATIONS: &[(i64, &str)] = &[
    // V1 → V2：wb_learned 补 auto_approve_enabled（REQ-018）。
    // 此前该 ALTER 硬编码在「版本匹配」分支且吞错；现为正式迁移项。
    (
        1,
        "ALTER TABLE wb_learned ADD COLUMN auto_approve_enabled INTEGER NOT NULL DEFAULT 1",
    ),
    // V2 → V3：Phase 11 Domain Agent Evolution（WB-DOC-03-V11.0）。
    // 新增 wb_evaluation（Event Log 派生结果，可重建）与 wb_mutation（提案 + Benchmark）。
    (
        2,
        "CREATE TABLE IF NOT EXISTS wb_evaluation (
  id                 TEXT PRIMARY KEY,
  run_id             TEXT NOT NULL,
  task_id            TEXT NOT NULL,
  skill_version      TEXT,
  correctness        REAL,
  completeness       REAL,
  efficiency         REAL,
  reliability        REAL,
  policy_compliance  REAL,
  output_quality     REAL,
  aggregate_score    REAL NOT NULL DEFAULT 0.0,
  failure_modes      TEXT NOT NULL DEFAULT '[]',
  evaluator_version  TEXT NOT NULL,
  created_at         TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_eval_run ON wb_evaluation(run_id);
CREATE INDEX IF NOT EXISTS idx_eval_task ON wb_evaluation(task_id);
CREATE TABLE IF NOT EXISTS wb_mutation (
  id                 TEXT PRIMARY KEY,
  target_asset       TEXT NOT NULL,
  source_version     TEXT NOT NULL,
  candidate_version  TEXT NOT NULL,
  root_cause         TEXT NOT NULL,
  evidence           TEXT NOT NULL DEFAULT '[]',
  delta              TEXT NOT NULL,
  validation_result  TEXT,
  benchmark_result   TEXT,
  status             TEXT NOT NULL,
  promotion_reason   TEXT,
  created_at         TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_mutation_status ON wb_mutation(status);
CREATE INDEX IF NOT EXISTS idx_mutation_target ON wb_mutation(target_asset);",
    ),
    // V3 → V4：Phase 11 11A 证据系统（WB-DOC-03-V11.0）。
    // wb_evaluation 加 evidence 列（每维度证据+置信度 JSON；Unknown ≠ Success 语义）。
    (
        3,
        "ALTER TABLE wb_evaluation ADD COLUMN evidence TEXT NOT NULL DEFAULT '{}'",
    ),
    // V4 → V5：Phase 11C 离线 Benchmark（WB-DOC-03-V11.0）。
    // 新增 wb_task（任务定义持久化：历史任务集 = 按 task_id 聚合的 wb_evaluation）。
    (
        4,
        "CREATE TABLE IF NOT EXISTS wb_task (
  task_id    TEXT PRIMARY KEY,
  definition TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
)",
    ),
    // V5 → V6：V2 Router Selection Learning（Trust 轴）。
    // wb_worker_stats：(capability, worker) 执行统计（派生层，可回放重建）。
    (
        5,
        "CREATE TABLE IF NOT EXISTS wb_worker_stats (
  capability       TEXT NOT NULL,
  worker_id        TEXT NOT NULL,
  runs             INTEGER NOT NULL DEFAULT 0,
  success          INTEGER NOT NULL DEFAULT 0,
  total_duration_ms INTEGER NOT NULL DEFAULT 0,
  updated_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (capability, worker_id)
)",
    ),
    // V6 → V7：V2 Router Selection Learning 可回放化。
    // wb_evaluation 加 worker_outcomes（per-worker 结果；WorkerStats 重建的数据源）。
    (
        6,
        "ALTER TABLE wb_evaluation ADD COLUMN worker_outcomes TEXT NOT NULL DEFAULT '[]'",
    ),
    // V7 → V8：S1 Decision Replay 前置——Worker Registry 持久化 + stats.first_seen。
    (
        7,
        "CREATE TABLE IF NOT EXISTS wb_worker (
  id           TEXT PRIMARY KEY,
  worker_type  TEXT NOT NULL,
  capabilities TEXT NOT NULL DEFAULT '[]',
  quality      REAL NOT NULL DEFAULT 0.0,
  trust        REAL NOT NULL DEFAULT 0.0,
  risk         INTEGER NOT NULL DEFAULT 1,
  cost         INTEGER NOT NULL DEFAULT 1,
  latency_ms   INTEGER NOT NULL DEFAULT 100,
  availability TEXT NOT NULL DEFAULT 'local',
  execution    TEXT NOT NULL DEFAULT 'Process',
  created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
ALTER TABLE wb_worker_stats ADD COLUMN created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP;",
    ),
    // V8 → V9（M1d）：V1 门面删除——wb_memory / wb_approval / wb_pattern·proposal·learned
    // / wb_approval_audit 随 V1 存储层退役（V2 认知状态归 CognitiveStore / Kernel 事实轨）。
    (
        8,
        "DROP TABLE IF EXISTS wb_memory; DROP TABLE IF EXISTS wb_approval; \
         DROP TABLE IF EXISTS wb_pattern; DROP TABLE IF EXISTS wb_proposal; \
         DROP TABLE IF EXISTS wb_learned; DROP TABLE IF EXISTS wb_approval_audit;",
    ),
    // V9 → V10（Phase 8）：Project Scope——项目维度世界模型。
    // wb_project 项目注册表（含本体定义）；对象/关系/事件/run 全部 project 归属
    //（NULL = 全局/历史，兼容旧库；世界按 project 过滤投影）。
    (
        9,
        "CREATE TABLE IF NOT EXISTS wb_project (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  ontology    TEXT NOT NULL DEFAULT '',
  status      TEXT NOT NULL DEFAULT 'active',
  created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
ALTER TABLE wb_ontology_object ADD COLUMN project_id TEXT;
ALTER TABLE wb_relation_edge ADD COLUMN project_id TEXT;
ALTER TABLE wb_trace_event ADD COLUMN project_id TEXT;
ALTER TABLE wb_run ADD COLUMN project_id TEXT;",
    ),
    (
        10,
        "CREATE TABLE IF NOT EXISTS wb_schema_migration (
  version    INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_schema_migration_applied_at
  ON wb_schema_migration (applied_at);",
    ),
    // V11 → V12（S1h）：审计轨迹物理 append-only；修正必须追加新事件。
    (
        11,
        "CREATE TRIGGER IF NOT EXISTS trg_wb_trace_event_append_only_update
BEFORE UPDATE ON wb_trace_event
BEGIN
  SELECT RAISE(ABORT, 'wb_trace_event is append-only');
END;
CREATE TRIGGER IF NOT EXISTS trg_wb_trace_event_append_only_delete
BEFORE DELETE ON wb_trace_event
BEGIN
  SELECT RAISE(ABORT, 'wb_trace_event is append-only');
END;",
    ),
    // V12 → V13（S2d）：审计事件完整性哈希链。旧事件由打开数据库时的
    // deterministic backfill 建链；若链表部分存在则 fail-closed，不静默修复。
    (
        12,
        "CREATE TABLE IF NOT EXISTS wb_audit_chain (
  sequence      INTEGER PRIMARY KEY,
  event_id      TEXT NOT NULL UNIQUE,
  previous_hash TEXT NOT NULL,
  hash          TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS trg_wb_audit_chain_append_only_update
BEFORE UPDATE ON wb_audit_chain
BEGIN
  SELECT RAISE(ABORT, 'wb_audit_chain is append-only');
END;
CREATE TRIGGER IF NOT EXISTS trg_wb_audit_chain_append_only_delete
BEFORE DELETE ON wb_audit_chain
BEGIN
  SELECT RAISE(ABORT, 'wb_audit_chain is append-only');
END;",
    ),
    // V13 → V14：Phase 2.5 自动运行引擎持久化任务队列。
    (
        13,
        "CREATE TABLE IF NOT EXISTS wb_automation_job (
  id            TEXT PRIMARY KEY,
  pack_id       TEXT NOT NULL,
  job_type      TEXT NOT NULL,
  dedupe_key    TEXT NOT NULL,
  payload       TEXT NOT NULL,
  status        TEXT NOT NULL,
  attempt       INTEGER NOT NULL DEFAULT 0,
  max_attempts  INTEGER NOT NULL DEFAULT 3,
  available_at  TEXT NOT NULL,
  started_at    TEXT,
  finished_at   TEXT,
  last_error    TEXT,
  result        TEXT,
  created_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE (pack_id, job_type, dedupe_key)
);
CREATE INDEX IF NOT EXISTS idx_automation_job_status_available
  ON wb_automation_job (status, available_at, created_at);",
    ),
    // V14 → V15：知识本体版本、演化提案、人工评审与回滚历史持久化。
    (
        14,
        "CREATE TABLE IF NOT EXISTS wb_ontology_version (
  revision    INTEGER PRIMARY KEY AUTOINCREMENT,
  id          TEXT NOT NULL UNIQUE,
  parent_id   TEXT,
  ontology    TEXT NOT NULL,
  runtime_ir  TEXT,
  created_at  TEXT NOT NULL,
  FOREIGN KEY (parent_id) REFERENCES wb_ontology_version(id)
);
CREATE INDEX IF NOT EXISTS idx_ontology_version_parent
  ON wb_ontology_version (parent_id, revision);
CREATE TABLE IF NOT EXISTS wb_ontology_proposal (
  id                 TEXT PRIMARY KEY,
  source_version_id  TEXT NOT NULL REFERENCES wb_ontology_version(id),
  proposal_json      TEXT NOT NULL,
  review_json        TEXT NOT NULL,
  candidate_runtime_ir TEXT,
  status             TEXT NOT NULL,
  applied_version_id TEXT REFERENCES wb_ontology_version(id),
  created_at         TEXT NOT NULL,
  reviewed_at        TEXT
);
CREATE INDEX IF NOT EXISTS idx_ontology_proposal_status
  ON wb_ontology_proposal (status, created_at);",
    ),
    // V15 → V16：企业共享资源预留账本。事务化写入是跨实例预算仲裁的事实源。
    (
        15,
        "CREATE TABLE IF NOT EXISTS wb_shared_resource_usage (
  id             TEXT NOT NULL,
  project_id     TEXT NOT NULL,
  resource_name  TEXT NOT NULL,
  amount         REAL NOT NULL,
  executed_at    TEXT NOT NULL,
  created_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (id, resource_name)
);
CREATE INDEX IF NOT EXISTS idx_shared_resource_window
  ON wb_shared_resource_usage (resource_name, executed_at);",
    ),
];

const MIGRATION_NAMES: &[(i64, &str)] = &[
    (1, "baseline"),
    (2, "learned_auto_approve_enabled"),
    (3, "evaluation_and_mutation"),
    (4, "evaluation_evidence"),
    (5, "task_definition"),
    (6, "worker_stats"),
    (7, "evaluation_worker_outcomes"),
    (8, "worker_registry"),
    (9, "retire_v1_storage_tables"),
    (10, "project_scope"),
    (11, "schema_migration_history"),
    (12, "trace_event_append_only"),
    (13, "audit_integrity_hash_chain"),
    (14, "automation_job_queue"),
    (15, "ontology_evolution_persistence"),
    (16, "shared_resource_ledger"),
];

const MIGRATION_DOWNS: &[(i64, &str)] = &[
    (1, "ALTER TABLE wb_learned DROP COLUMN auto_approve_enabled"),
    (
        2,
        "DROP INDEX IF EXISTS idx_eval_run; DROP INDEX IF EXISTS idx_eval_task; \
         DROP INDEX IF EXISTS idx_mutation_status; DROP INDEX IF EXISTS idx_mutation_target; \
         DROP TABLE IF EXISTS wb_mutation; DROP TABLE IF EXISTS wb_evaluation;",
    ),
    (3, "ALTER TABLE wb_evaluation DROP COLUMN evidence"),
    (4, "DROP TABLE IF EXISTS wb_task"),
    (5, "DROP TABLE IF EXISTS wb_worker_stats"),
    (6, "ALTER TABLE wb_evaluation DROP COLUMN worker_outcomes"),
    (
        7,
        "DROP TABLE IF EXISTS wb_worker; ALTER TABLE wb_worker_stats DROP COLUMN created_at",
    ),
    (
        8,
        "CREATE TABLE IF NOT EXISTS wb_memory (id TEXT PRIMARY KEY); \
         CREATE TABLE IF NOT EXISTS wb_approval (id TEXT PRIMARY KEY); \
         CREATE TABLE IF NOT EXISTS wb_pattern (id TEXT PRIMARY KEY); \
         CREATE TABLE IF NOT EXISTS wb_proposal (id TEXT PRIMARY KEY); \
         CREATE TABLE IF NOT EXISTS wb_learned (
           id TEXT PRIMARY KEY, kind TEXT NOT NULL, action TEXT NOT NULL,
           confidence REAL NOT NULL, proposal_id TEXT NOT NULL, run_ids TEXT NOT NULL,
           created_at TEXT NOT NULL,
           auto_approve_enabled INTEGER NOT NULL DEFAULT 1
         ); \
         CREATE TABLE IF NOT EXISTS wb_approval_audit (id TEXT PRIMARY KEY)",
    ),
    (
        9,
        "DROP INDEX IF EXISTS idx_trace_project; \
         ALTER TABLE wb_ontology_object DROP COLUMN project_id; \
         ALTER TABLE wb_relation_edge DROP COLUMN project_id; \
         ALTER TABLE wb_trace_event DROP COLUMN project_id; \
         ALTER TABLE wb_run DROP COLUMN project_id; \
         DROP TABLE IF EXISTS wb_project",
    ),
    (
        10,
        "DROP INDEX IF EXISTS idx_schema_migration_applied_at; DROP TABLE IF EXISTS wb_schema_migration",
    ),
    (
        11,
        "DROP TRIGGER IF EXISTS trg_wb_trace_event_append_only_update;
DROP TRIGGER IF EXISTS trg_wb_trace_event_append_only_delete",
    ),
    (
        12,
        "DROP TRIGGER IF EXISTS trg_wb_audit_chain_append_only_update;
DROP TRIGGER IF EXISTS trg_wb_audit_chain_append_only_delete;
DROP TABLE IF EXISTS wb_audit_chain",
    ),
    (
        13,
        "DROP INDEX IF EXISTS idx_automation_job_status_available; DROP TABLE IF EXISTS wb_automation_job",
    ),
    (
        14,
        "DROP TABLE IF EXISTS wb_ontology_proposal; DROP TABLE IF EXISTS wb_ontology_version",
    ),
    (
        15,
        "DROP INDEX IF EXISTS idx_shared_resource_window; \
         DROP TABLE IF EXISTS wb_shared_resource_usage",
    ),
];

fn migration_name(target_version: i64) -> Option<&'static str> {
    MIGRATION_NAMES
        .iter()
        .find(|(version, _)| *version == target_version)
        .map(|(_, name)| *name)
}

fn migration_down(from_version: i64) -> Option<&'static str> {
    MIGRATION_DOWNS
        .iter()
        .find(|(from, _)| *from == from_version)
        .map(|(_, sql)| *sql)
}

fn constraint(message: impl Into<String>) -> StorageError {
    StorageError::Constraint {
        code: "WB2102",
        message: message.into(),
    }
}

fn is_no_such_table(e: &libsql::Error) -> bool {
    e.to_string().to_ascii_lowercase().contains("no such table")
}

async fn read_version(conn: &Connection) -> Result<i64, StorageError> {
    let mut rows = conn
        .query(
            "SELECT value FROM wb_meta WHERE key='schema_version'",
            params![],
        )
        .await
        .map_err(|e| {
            if is_no_such_table(&e) {
                StorageError::Constraint {
                    code: "WB2101",
                    message: "数据库缺少 wb_meta.schema_version；请先执行受支持的 schema 迁移"
                        .into(),
                }
            } else {
                StorageError::Db(e)
            }
        })?;
    let Some(row) = rows.next().await? else {
        return Err(StorageError::Constraint {
            code: "WB2101",
            message: "数据库缺少 wb_meta.schema_version；请先执行受支持的 schema 迁移".into(),
        });
    };
    let raw: String = row.get(0)?;
    raw.parse::<i64>().map_err(|_| StorageError::Constraint {
        code: "WB2101",
        message: format!("无法识别的 schema 版本: {raw}"),
    })
}

async fn write_version(conn: &libsql::Transaction, version: i64) -> Result<(), StorageError> {
    conn.execute(
        "INSERT OR REPLACE INTO wb_meta (key, value) VALUES ('schema_version', ?1)",
        params![version.to_string()],
    )
    .await?;
    Ok(())
}

async fn table_exists(conn: &Connection, name: &str) -> Result<bool, StorageError> {
    let mut rows = conn
        .query(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
            params![name],
        )
        .await?;
    Ok(rows.next().await?.is_some())
}

async fn table_exists_tx(conn: &libsql::Transaction, name: &str) -> Result<bool, StorageError> {
    let mut rows = conn
        .query(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
            params![name],
        )
        .await?;
    Ok(rows.next().await?.is_some())
}

async fn seed_migration_history(conn: &Connection) -> Result<(), StorageError> {
    let mut rows = conn
        .query(
            "SELECT version, name FROM wb_schema_migration ORDER BY version",
            params![],
        )
        .await?;
    let mut actual = BTreeMap::new();
    while let Some(row) = rows.next().await? {
        actual.insert(row.get::<i64>(0)?, row.get::<String>(1)?);
    }

    let expected: BTreeMap<i64, String> = MIGRATION_NAMES
        .iter()
        .map(|(version, name)| (*version, (*name).to_string()))
        .collect();
    for (version, name) in &actual {
        if expected.get(version) != Some(name) {
            return Err(constraint(format!(
                "迁移历史与程序不一致：实际={actual:?}，期望={expected:?}"
            )));
        }
    }
    for (version, name) in &expected {
        if !actual.contains_key(version) {
            conn.execute(
                "INSERT INTO wb_schema_migration (version, name) VALUES (?1, ?2)",
                params![*version, name.clone()],
            )
            .await?;
        }
    }
    Ok(())
}

async fn record_migration(
    tx: &libsql::Transaction,
    target_version: i64,
) -> Result<(), StorageError> {
    let Some(name) = migration_name(target_version) else {
        return Err(constraint(format!("缺少迁移 {target_version}")));
    };
    if table_exists_tx(tx, "wb_schema_migration").await? {
        tx.execute(
            "INSERT OR REPLACE INTO wb_schema_migration (version, name) VALUES (?1, ?2)",
            params![target_version, name],
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn migrate_to(
    conn: &Connection,
    current: i64,
    target: i64,
) -> Result<(), StorageError> {
    if current == target {
        return Ok(());
    }
    let tx = conn.transaction().await?;
    let mut version = current;
    if target > current {
        while version < target {
            let Some((from, sql)) = MIGRATIONS.iter().find(|(from, _)| *from == version) else {
                return Err(constraint(format!("缺少 {version} → {} 迁移", version + 1)));
            };
            tx.execute_batch(sql).await?;
            version = *from + 1;
            write_version(&tx, version).await?;
            record_migration(&tx, version).await?;
        }
    } else {
        while version > target {
            let from = version - 1;
            let Some(sql) = migration_down(from) else {
                return Err(constraint(format!("缺少 {} → {version} 回滚", from)));
            };
            if table_exists_tx(&tx, "wb_schema_migration").await? {
                tx.execute(
                    "DELETE FROM wb_schema_migration WHERE version=?1",
                    params![version],
                )
                .await?;
            }
            tx.execute_batch(sql).await?;
            version = from;
            write_version(&tx, version).await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

type ColumnSignature = (String, String, i64, i64, String);

#[derive(Debug, Clone, PartialEq, Eq)]
struct SchemaSnapshot {
    objects: BTreeMap<String, String>,
    columns: BTreeMap<String, Vec<ColumnSignature>>,
}

async fn snapshot(conn: &Connection) -> Result<SchemaSnapshot, StorageError> {
    let mut rows = conn
        .query(
            "SELECT type, name, COALESCE(sql, '') FROM sqlite_master \
             WHERE name != 'sqlite_sequence' ORDER BY type, name",
            params![],
        )
        .await?;
    let mut objects = BTreeMap::new();
    let mut table_names = Vec::new();
    while let Some(row) = rows.next().await? {
        let kind: String = row.get(0)?;
        let name: String = row.get(1)?;
        let sql: String = row.get(2)?;
        let signature = if kind == "table" {
            String::new()
        } else {
            normalize_sql(&sql)
        };
        objects.insert(format!("{kind}:{name}"), signature);
        if kind == "table" {
            table_names.push(name);
        }
    }

    let mut columns = BTreeMap::new();
    for name in table_names {
        let escaped = name.replace('"', "\"\"");
        let mut table_rows = conn
            .query(&format!("PRAGMA table_info(\"{escaped}\")"), params![])
            .await?;
        let mut table_columns = Vec::new();
        while let Some(row) = table_rows.next().await? {
            table_columns.push((
                row.get::<String>(1)?,
                row.get::<String>(2)?,
                row.get::<i64>(3)?,
                row.get::<i64>(5)?,
                row.get::<Option<String>>(4)?.unwrap_or_default(),
            ));
        }
        table_columns.sort_by(|left, right| left.0.cmp(&right.0));
        columns.insert(name, table_columns);
    }
    Ok(SchemaSnapshot { objects, columns })
}

fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

async fn canonical_snapshot() -> Result<SchemaSnapshot, StorageError> {
    let db = libsql::Builder::new_local(":memory:")
        .build()
        .await
        .map_err(StorageError::Db)?;
    let conn = db.connect().map_err(StorageError::Db)?;
    conn.execute_batch(&codegen::canonical_schema()).await?;
    snapshot(&conn).await
}

pub(crate) async fn validate_schema(conn: &Connection) -> Result<(), StorageError> {
    let expected = canonical_snapshot().await?;
    let actual = snapshot(conn).await?;
    if actual != expected {
        let object_diff: Vec<_> = actual
            .objects
            .iter()
            .filter(|(key, value)| expected.objects.get(*key) != Some(*value))
            .map(|(key, value)| (key, value, expected.objects.get(key)))
            .collect();
        let column_diff: Vec<_> = actual
            .columns
            .iter()
            .filter(|(key, value)| expected.columns.get(*key) != Some(*value))
            .map(|(key, value)| (key, value, expected.columns.get(key)))
            .collect();
        return Err(constraint(format!(
            "数据库 schema 与 canonical_schema 不一致：objects={object_diff:?}, columns={column_diff:?}"
        )));
    }
    Ok(())
}

pub(crate) async fn ensure_schema(conn: &Connection) -> Result<(), StorageError> {
    let has_obj = match conn
        .query("SELECT id FROM wb_ontology_object LIMIT 0", params![])
        .await
    {
        Ok(_) => true,
        Err(e) if is_no_such_table(&e) => false,
        Err(e) => return Err(StorageError::Db(e)),
    };

    if !has_obj {
        conn.execute_batch(&codegen::canonical_schema()).await?;
        conn.execute(
            "INSERT OR REPLACE INTO wb_meta (key, value) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )
        .await?;
        seed_migration_history(conn).await?;
        return validate_schema(conn).await;
    }

    let current = read_version(conn).await?;
    if current > SCHEMA_VERSION {
        return Err(StorageError::Constraint {
            code: "WB2101",
            message: format!(
                "数据库 schema 版本 {current} 高于程序支持的 {SCHEMA_VERSION}（请升级程序后再打开）"
            ),
        });
    }
    if current < 1 {
        return Err(constraint(format!("不支持的 schema 版本 {current}")));
    }

    if current < SCHEMA_VERSION {
        migrate_to(conn, current, SCHEMA_VERSION).await?;
        conn.execute_batch(&codegen::canonical_schema()).await?;
    }

    if !table_exists(conn, "wb_schema_migration").await? {
        return Err(constraint("缺少 wb_schema_migration 迁移历史表"));
    }
    seed_migration_history(conn).await?;
    validate_schema(conn).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fresh_schema_has_complete_migration_history_and_matches_canonical() {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .unwrap();
        let conn = db.connect().unwrap();
        ensure_schema(&conn).await.unwrap();
        let mut rows = conn
            .query(
                "SELECT version, name FROM wb_schema_migration ORDER BY version",
                params![],
            )
            .await
            .unwrap();
        let mut versions = Vec::new();
        while let Some(row) = rows.next().await.unwrap() {
            versions.push((row.get::<i64>(0).unwrap(), row.get::<String>(1).unwrap()));
        }
        assert_eq!(versions.len(), SCHEMA_VERSION as usize);
        assert_eq!(versions.first().unwrap().0, 1);
        assert_eq!(versions.last().unwrap().0, SCHEMA_VERSION);
        validate_schema(&conn).await.unwrap();
    }

    #[tokio::test]
    async fn trace_event_is_physically_append_only_and_migration_restores_triggers() {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .unwrap();
        let conn = db.connect().unwrap();
        ensure_schema(&conn).await.unwrap();

        let mut triggers = conn
            .query(
                "SELECT name FROM sqlite_master WHERE type='trigger' AND name LIKE 'trg_wb_trace_event_append_only_%' ORDER BY name",
                params![],
            )
            .await
            .unwrap();
        let mut names = Vec::new();
        while let Some(row) = triggers.next().await.unwrap() {
            names.push(row.get::<String>(0).unwrap());
        }
        assert_eq!(
            names,
            vec![
                "trg_wb_trace_event_append_only_delete",
                "trg_wb_trace_event_append_only_update",
            ]
        );

        conn.execute(
            "INSERT INTO wb_trace_event (id, kind, event_type, object_refs, timestamp) VALUES (?1, ?2, ?3, ?4, ?5)",
            params!["audit-1", "world", "ObjectCreated", "[\"object-1\"]", "1"],
        )
        .await
        .unwrap();
        let update = conn
            .execute(
                "UPDATE wb_trace_event SET event_type='Tampered' WHERE id='audit-1'",
                params![],
            )
            .await;
        assert!(
            update.is_err(),
            "审计轨迹 UPDATE 必须被 SQLite trigger 拒绝"
        );
        let delete = conn
            .execute("DELETE FROM wb_trace_event WHERE id='audit-1'", params![])
            .await;
        assert!(
            delete.is_err(),
            "审计轨迹 DELETE 必须被 SQLite trigger 拒绝"
        );

        let mut rows = conn
            .query(
                "SELECT event_type FROM wb_trace_event WHERE id='audit-1'",
                params![],
            )
            .await
            .unwrap();
        assert_eq!(
            rows.next()
                .await
                .unwrap()
                .unwrap()
                .get::<String>(0)
                .unwrap(),
            "ObjectCreated"
        );
        drop(rows);

        migrate_to(&conn, SCHEMA_VERSION, 11).await.unwrap();
        let mut dropped = conn
            .query(
                "SELECT name FROM sqlite_master WHERE type='trigger' AND name LIKE 'trg_wb_trace_event_append_only_%'",
                params![],
            )
            .await
            .unwrap();
        assert!(dropped.next().await.unwrap().is_none());
        drop(dropped);

        migrate_to(&conn, 11, SCHEMA_VERSION).await.unwrap();
        validate_schema(&conn).await.unwrap();
        let update = conn
            .execute(
                "UPDATE wb_trace_event SET event_type='TamperedAgain' WHERE id='audit-1'",
                params![],
            )
            .await;
        assert!(update.is_err(), "迁移恢复后 UPDATE 仍必须被拒绝");
    }

    #[tokio::test]
    async fn current_schema_drift_is_rejected_instead_of_repaired() {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .unwrap();
        let conn = db.connect().unwrap();
        ensure_schema(&conn).await.unwrap();
        conn.execute("DROP INDEX idx_obj_type", params![])
            .await
            .unwrap();
        let err = ensure_schema(&conn).await.unwrap_err();
        match err {
            StorageError::Constraint { code, .. } => assert_eq!(code, "WB2102"),
            other => panic!("应返回 WB2102，实际: {other:?}"),
        }
    }

    #[tokio::test]
    async fn every_registered_migration_supports_full_down_and_up_roundtrip() {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .unwrap();
        let conn = db.connect().unwrap();
        ensure_schema(&conn).await.unwrap();
        migrate_to(&conn, SCHEMA_VERSION, 1).await.unwrap();
        migrate_to(&conn, 1, SCHEMA_VERSION).await.unwrap();
        conn.execute_batch(&codegen::canonical_schema())
            .await
            .unwrap();
        seed_migration_history(&conn).await.unwrap();
        validate_schema(&conn).await.unwrap();
    }

    #[tokio::test]
    async fn failed_migration_rolls_back_version_and_ddl() {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .unwrap();
        let conn = db.connect().unwrap();
        ensure_schema(&conn).await.unwrap();
        // 回滚到 V10 会同时移除迁移历史表；失败的 V10→V14 迁移必须整体回滚。
        let baseline_without_history = 10;
        migrate_to(&conn, SCHEMA_VERSION, baseline_without_history)
            .await
            .unwrap();
        assert!(
            migrate_to(&conn, baseline_without_history, SCHEMA_VERSION + 1)
                .await
                .is_err()
        );
        let mut rows = conn
            .query(
                "SELECT value FROM wb_meta WHERE key='schema_version'",
                params![],
            )
            .await
            .unwrap();
        let version: String = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(version, baseline_without_history.to_string());
        drop(rows);
        assert!(!table_exists(&conn, "wb_schema_migration").await.unwrap());
    }

    #[tokio::test]
    async fn migration_up_and_down_are_transactional() {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .unwrap();
        let conn = db.connect().unwrap();
        ensure_schema(&conn).await.unwrap();
        // V10 是最后一个没有迁移历史表的状态，用于验证 DDL 与版本写入的原子性。
        let baseline_without_history = 10;
        migrate_to(&conn, SCHEMA_VERSION, baseline_without_history)
            .await
            .unwrap();
        let mut rows = conn
            .query(
                "SELECT value FROM wb_meta WHERE key='schema_version'",
                params![],
            )
            .await
            .unwrap();
        let version: String = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(version, baseline_without_history.to_string());
        drop(rows);
        assert!(!table_exists(&conn, "wb_schema_migration").await.unwrap());
        migrate_to(&conn, baseline_without_history, SCHEMA_VERSION)
            .await
            .unwrap();
        conn.execute_batch(&codegen::canonical_schema())
            .await
            .unwrap();
        seed_migration_history(&conn).await.unwrap();
        validate_schema(&conn).await.unwrap();
    }
}
