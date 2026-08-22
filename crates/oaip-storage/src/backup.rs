//! Local-first database backup and restore primitives.
//!
//! Backups use SQLite/libSQL's `VACUUM INTO` so the snapshot is produced by the
//! database engine rather than by copying a live database file.  Restore first
//! validates the snapshot, then installs it with a same-directory rename.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use libsql::{Connection, params};

use crate::{StorageError, schema};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

const BACKUP_EXISTS: &str = "WB2201";
const BACKUP_INVALID: &str = "WB2202";
const BACKUP_SAME_PATH: &str = "WB2203";

pub(crate) async fn backup(
    conn: &Connection,
    destination: impl AsRef<Path>,
) -> Result<(), StorageError> {
    let destination = destination.as_ref();
    ensure_destination_parent(destination)?;
    if destination.exists() {
        return Err(constraint(
            BACKUP_EXISTS,
            format!("备份目标已存在，不覆盖已有备份: {}", destination.display()),
        ));
    }

    let temporary = temporary_path(destination);
    let result = async {
        let sql_path = sqlite_path(&temporary)?;
        conn.execute(&format!("VACUUM INTO '{sql_path}'"), params![])
            .await?;
        if !temporary.is_file() {
            return Err(constraint(
                BACKUP_INVALID,
                "VACUUM INTO 未生成备份文件".to_string(),
            ));
        }
        std::fs::rename(&temporary, destination)?;
        Ok(())
    }
    .await;

    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

pub(crate) async fn restore(
    source: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> Result<(), StorageError> {
    let source = source.as_ref();
    let destination = destination.as_ref();
    if !source.is_file() {
        return Err(constraint(
            BACKUP_INVALID,
            format!("备份文件不存在或不是普通文件: {}", source.display()),
        ));
    }
    if same_path(source, destination)? {
        return Err(constraint(
            BACKUP_SAME_PATH,
            "restore 的源文件与目标数据库相同".to_string(),
        ));
    }

    validate_snapshot(source).await?;
    ensure_destination_parent(destination)?;

    let temporary = temporary_path(destination);
    let result = async {
        std::fs::copy(source, &temporary)?;
        remove_sidecar(destination, "-wal")?;
        remove_sidecar(destination, "-shm")?;
        std::fs::rename(&temporary, destination)?;
        Ok(())
    }
    .await;

    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

async fn validate_snapshot(path: &Path) -> Result<(), StorageError> {
    let path = path.to_str().ok_or_else(|| {
        constraint(
            BACKUP_INVALID,
            format!("备份路径不是有效 UTF-8: {}", path.display()),
        )
    })?;
    let db = libsql::Builder::new_local(path).build().await?;
    let conn = db.connect()?;
    // Do not call ensure_schema here: restore must never silently turn an empty
    // or malformed file into a valid database.  A legacy but structurally valid
    // snapshot is still rejected until an explicit migration is performed.
    conn.query("SELECT id FROM wb_ontology_object LIMIT 0", params![])
        .await
        .map_err(|e| StorageError::Constraint {
            code: BACKUP_INVALID,
            message: format!("备份缺少 OpenAIP-OS 核心表: {e}"),
        })?;
    schema::validate_schema(&conn).await
}

fn ensure_destination_parent(path: &Path) -> Result<(), StorageError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(constraint(
            BACKUP_INVALID,
            format!("目标目录不存在: {}", parent.display()),
        ));
    }
    Ok(())
}

fn same_path(source: &Path, destination: &Path) -> Result<bool, StorageError> {
    if !destination.exists() {
        let source = std::fs::canonicalize(source)?;
        let parent = destination.parent().unwrap_or_else(|| Path::new("."));
        let parent = std::fs::canonicalize(parent)?;
        return Ok(source
            == parent.join(destination.file_name().ok_or_else(|| {
                constraint(BACKUP_INVALID, "目标数据库路径缺少文件名".to_string())
            })?));
    }
    Ok(std::fs::canonicalize(source)? == std::fs::canonicalize(destination)?)
}

fn remove_sidecar(path: &Path, suffix: &str) -> Result<(), StorageError> {
    let sidecar = PathBuf::from(format!("{}{}", path.display(), suffix));
    if sidecar.exists() {
        std::fs::remove_file(sidecar)?;
    }
    Ok(())
}

fn temporary_path(destination: &Path) -> PathBuf {
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = destination
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("database");
    destination.with_file_name(format!(".{name}.oaip-tmp-{}-{n}", std::process::id()))
}

fn sqlite_path(path: &Path) -> Result<String, StorageError> {
    let path = path.to_str().ok_or_else(|| {
        constraint(
            BACKUP_INVALID,
            format!("备份路径不是有效 UTF-8: {}", path.display()),
        )
    })?;
    Ok(path.replace('\'', "''"))
}

fn constraint(code: &'static str, message: String) -> StorageError {
    StorageError::Constraint { code, message }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LibSQLStorage, ObjectEngine, WorldCommand};
    use serde_json::json;

    fn path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("oaip_{name}_{}", std::process::id()))
    }

    fn cleanup(path: &Path) {
        for suffix in ["", "-wal", "-shm"] {
            let p = PathBuf::from(format!("{}{}", path.display(), suffix));
            let _ = std::fs::remove_file(p);
        }
    }

    #[tokio::test]
    async fn backup_is_consistent_and_does_not_overwrite() {
        let db_path = path("backup_source");
        let backup_path = path("backup_target");
        cleanup(&db_path);
        cleanup(&backup_path);

        let storage = LibSQLStorage::open(db_path.to_str().unwrap())
            .await
            .unwrap();
        storage
            .apply_world(WorldCommand::CreateObject {
                object_type: "Customer".into(),
                id: "backup-1".into(),
                data: json!({"name": "Ada"}),
                project_id: None,
            })
            .await
            .unwrap();
        storage.backup_to(&backup_path).await.unwrap();
        assert!(backup_path.is_file());
        let backup = LibSQLStorage::open(backup_path.to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(
            backup.get("backup-1").await.unwrap().unwrap().id,
            "backup-1"
        );
        let err = storage.backup_to(&backup_path).await.unwrap_err();
        assert!(matches!(
            err,
            StorageError::Constraint {
                code: BACKUP_EXISTS,
                ..
            }
        ));

        cleanup(&db_path);
        cleanup(&backup_path);
    }

    #[tokio::test]
    async fn restore_replaces_database_atomically_after_snapshot_validation() {
        let source_path = path("restore_source");
        let backup_path = path("restore_backup");
        let destination_path = path("restore_destination");
        cleanup(&source_path);
        cleanup(&backup_path);
        cleanup(&destination_path);

        let source = LibSQLStorage::open(source_path.to_str().unwrap())
            .await
            .unwrap();
        source
            .apply_world(WorldCommand::CreateObject {
                object_type: "Customer".into(),
                id: "restore-1".into(),
                data: json!({"name": "Grace"}),
                project_id: None,
            })
            .await
            .unwrap();
        source.backup_to(&backup_path).await.unwrap();

        let destination = LibSQLStorage::open(destination_path.to_str().unwrap())
            .await
            .unwrap();
        destination
            .apply_world(WorldCommand::CreateObject {
                object_type: "Customer".into(),
                id: "old".into(),
                data: json!({"name": "Old"}),
                project_id: None,
            })
            .await
            .unwrap();
        drop(destination);

        restore(&backup_path, &destination_path).await.unwrap();
        let restored = LibSQLStorage::open(destination_path.to_str().unwrap())
            .await
            .unwrap();
        assert!(restored.get("restore-1").await.unwrap().is_some());
        assert!(restored.get("old").await.unwrap().is_none());

        cleanup(&source_path);
        cleanup(&backup_path);
        cleanup(&destination_path);
    }

    #[tokio::test]
    async fn restore_rejects_malformed_snapshot_without_touching_destination() {
        let source_path = path("restore_bad_source");
        let destination_path = path("restore_bad_destination");
        cleanup(&source_path);
        cleanup(&destination_path);
        std::fs::write(&source_path, b"not a sqlite database").unwrap();

        let destination = LibSQLStorage::open(destination_path.to_str().unwrap())
            .await
            .unwrap();
        destination
            .apply_world(WorldCommand::CreateObject {
                object_type: "Customer".into(),
                id: "keep".into(),
                data: json!({"name": "Keep"}),
                project_id: None,
            })
            .await
            .unwrap();
        drop(destination);

        let err = restore(&source_path, &destination_path).await.unwrap_err();
        assert!(matches!(
            err,
            StorageError::Constraint {
                code: BACKUP_INVALID,
                ..
            }
        ));
        let unchanged = LibSQLStorage::open(destination_path.to_str().unwrap())
            .await
            .unwrap();
        assert!(unchanged.get("keep").await.unwrap().is_some());

        cleanup(&source_path);
        cleanup(&destination_path);
    }
}
