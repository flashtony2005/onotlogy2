//! DSH/Fabric 任务协议与终局结果的最小结构化消费层。
//!
//! 这里不解析具体 Agent 的私有协议；只负责统一消息 payload 中的任务元数据、
//! workspace 变更摘要和常见测试输出摘要。进程输出本身仍由 ProcessAdapter 原样保留。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::worker::process::ProcessExecutionContext;

const IGNORED_DIRECTORIES: &[&str] = &[".git", "target", ".oaip", ".tmp", "tmp"];

/// 校验并规范化 Phase 2.2 的结构化任务描述。
///
/// task 仍表示传给进程的纯文本任务；结构化任务必须使用 task_spec，
/// 避免同一个字段同时承担命令参数和任务语义。
pub fn validate_payload(payload: &Value) -> Result<Value, String> {
    if let Some(args) = payload.get("args") {
        let Some(items) = args.as_array() else {
            return Err("任务协议无效: args 必须是字符串数组".to_string());
        };
        if items.iter().any(|item| !item.is_string()) {
            return Err("任务协议无效: args 必须是字符串数组".to_string());
        }
    }

    if payload.get("task").is_some_and(|task| !task.is_string()) {
        return Err("任务协议无效: task 必须是字符串；结构化任务使用 task_spec".to_string());
    }

    let Some(raw) = payload.get("task_spec") else {
        return Ok(Value::Null);
    };
    if payload.get("task").is_some() {
        return Err("任务协议无效: task 与 task_spec 不能同时提供".to_string());
    }
    let Some(object) = raw.as_object() else {
        return Err("任务协议无效: task_spec 必须是对象".to_string());
    };
    let description = object
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "任务协议无效: task_spec.description 不能为空".to_string())?;
    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "任务协议无效: task_spec.kind 不能为空".to_string())?;
    if let Some(acceptance) = object.get("acceptance")
        && !acceptance.is_object()
    {
        return Err("任务协议无效: task_spec.acceptance 必须是对象".to_string());
    }

    let mut normalized = json!({
        "description": description,
        "kind": kind,
    });
    if let Some(acceptance) = object.get("acceptance") {
        normalized["acceptance"] = acceptance.clone();
    }
    Ok(normalized)
}

/// 从纯文本任务或结构化任务中提取传给 CLI 的任务文本。
pub fn task_text(payload: &Value) -> Option<String> {
    payload
        .get("task")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            payload
                .get("task_spec")
                .and_then(|value| value.get("description"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceSnapshot {
    available: bool,
    files: BTreeMap<String, String>,
}

/// 在执行前取得 workspace 文件快照。只记录相对路径和 SHA-256，不暴露文件内容。
pub fn workspace_snapshot(context: &ProcessExecutionContext) -> WorkspaceSnapshot {
    let Some(root) = context.workspace_root() else {
        return WorkspaceSnapshot::default();
    };
    // /tmp 是宿主共享临时目录，不是可审计的任务 workspace；扫描它会把取消窗口
    // 拉长到不可接受。调用方应传入独立目录，真实 isolated workspace 仍完整递归扫描。
    let temp_dir = std::env::temp_dir();
    let canonical_temp = std::fs::canonicalize(&temp_dir).ok();
    if root == temp_dir.as_path()
        || canonical_temp.as_deref() == Some(root)
        || root == Path::new("/tmp")
        || root == Path::new("/private/tmp")
    {
        return WorkspaceSnapshot::default();
    }
    let mut files = BTreeMap::new();
    if collect_files(root, root, &mut files).is_err() {
        return WorkspaceSnapshot::default();
    }
    WorkspaceSnapshot {
        available: true,
        files,
    }
}

/// 比较执行前后快照，生成稳定、可直接写入 task.result 的 JSON。
pub fn workspace_diff(before: &WorkspaceSnapshot, context: &ProcessExecutionContext) -> Value {
    if !before.available {
        return unavailable_workspace_diff();
    }
    let after = workspace_snapshot(context);
    if !after.available {
        return unavailable_workspace_diff();
    }

    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut deleted = Vec::new();
    for (path, hash) in &after.files {
        match before.files.get(path) {
            None => added.push(path.clone()),
            Some(previous) if previous != hash => modified.push(path.clone()),
            _ => {}
        }
    }
    for path in before.files.keys() {
        if !after.files.contains_key(path) {
            deleted.push(path.clone());
        }
    }
    json!({
        "available": true,
        "added": added,
        "modified": modified,
        "deleted": deleted,
        "counts": {
            "added": added.len(),
            "modified": modified.len(),
            "deleted": deleted.len(),
        }
    })
}

pub fn unavailable_workspace_diff() -> Value {
    json!({
        "available": false,
        "added": [],
        "modified": [],
        "deleted": [],
        "counts": { "added": 0, "modified": 0, "deleted": 0 }
    })
}

fn collect_files(
    root: &Path,
    current: &Path,
    files: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(current)
        .map_err(|error| format!("读取 workspace 失败: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取 workspace 项失败: {error}"))?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|error| format!("读取 workspace 类型失败: {error}"))?;
        let name = entry.file_name();
        if file_type.is_dir() {
            if name
                .to_str()
                .is_some_and(|value| IGNORED_DIRECTORIES.contains(&value))
            {
                continue;
            }
            collect_files(root, &entry.path(), files)?;
        } else if file_type.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|error| format!("计算 workspace 相对路径失败: {error}"))?
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = fs::read(entry.path())
                .map_err(|error| format!("读取 workspace 文件失败: {error}"))?;
            let mut hasher = Sha256::new();
            hasher.update(bytes);
            files.insert(relative, format!("{:x}", hasher.finalize()));
        }
    }
    Ok(())
}

/// 从 dsh/测试工具的 JSON 或常见文本输出中提取轻量测试统计。
pub fn test_summary(stdout: &str) -> Value {
    let trimmed = stdout.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed)
        && let Some(summary) = value.get("test_summary")
        && let Some(object) = summary.as_object()
        && ["passed", "failed", "skipped", "total", "status"]
            .iter()
            .any(|key| object.contains_key(*key))
    {
        let passed = count_value(object.get("passed"));
        let failed = count_value(object.get("failed"));
        let skipped = count_value(object.get("skipped").or_else(|| object.get("ignored")));
        let total = count_value(object.get("total"))
            .unwrap_or(passed.unwrap_or(0) + failed.unwrap_or(0) + skipped.unwrap_or(0));
        let status = object
            .get("status")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| summary_status(failed.unwrap_or(0), total).to_string());
        return json!({
            "detected": true,
            "passed": passed.unwrap_or(0),
            "failed": failed.unwrap_or(0),
            "skipped": skipped.unwrap_or(0),
            "total": total,
            "status": status,
        });
    }

    for line in stdout.lines() {
        let mut passed = None;
        let mut failed = None;
        let mut skipped = None;
        for (label, slot) in [
            ("passed", &mut passed),
            ("failed", &mut failed),
            ("skipped", &mut skipped),
        ] {
            if let Some(index) = line.find(label) {
                *slot = number_before(&line[..index]);
            }
        }
        if let Some(index) = line.find("ignored") {
            skipped = number_before(&line[..index]);
        }
        if passed.is_some() || failed.is_some() || skipped.is_some() {
            let passed = passed.unwrap_or(0);
            let failed = failed.unwrap_or(0);
            let skipped = skipped.unwrap_or(0);
            let total = passed + failed + skipped;
            return json!({
                "detected": true,
                "passed": passed,
                "failed": failed,
                "skipped": skipped,
                "total": total,
                "status": summary_status(failed, total),
            });
        }
    }
    json!({ "detected": false })
}

fn count_value(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64)
}

fn number_before(prefix: &str) -> Option<u64> {
    prefix
        .rsplit(|character: char| !character.is_ascii_digit())
        .find(|part| !part.is_empty())
        .and_then(|part| part.parse().ok())
}

fn summary_status(failed: u64, total: u64) -> &'static str {
    if failed > 0 {
        "failed"
    } else if total > 0 {
        "passed"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_and_normalizes_structured_task() {
        let spec = validate_payload(&json!({
            "task_spec": {
                "description": "修复金额计算",
                "kind": "code_change",
                "acceptance": { "run_tests": true }
            }
        }))
        .unwrap();
        assert_eq!(spec["kind"], "code_change");
        assert_eq!(spec["acceptance"]["run_tests"], true);
    }

    #[test]
    fn rejects_ambiguous_task_protocol() {
        let error = validate_payload(&json!({
            "task": "text",
            "task_spec": { "description": "structured", "kind": "code_change" }
        }))
        .unwrap_err();
        assert!(error.contains("不能同时"));
    }

    #[test]
    fn extracts_json_and_text_test_summaries() {
        assert_eq!(
            test_summary(r#"{"test_summary":{"passed":2,"failed":1,"skipped":0}}"#),
            json!({ "detected": true, "passed": 2, "failed": 1, "skipped": 0, "total": 3, "status": "failed" })
        );
        assert_eq!(
            test_summary("test result: ok. 12 passed; 0 failed; 2 ignored"),
            json!({ "detected": true, "passed": 12, "failed": 0, "skipped": 2, "total": 14, "status": "passed" })
        );
        assert_eq!(test_summary("hello"), json!({ "detected": false }));
    }

    #[test]
    fn computes_workspace_diff_without_exposing_contents() {
        let root = std::env::temp_dir().join(format!("oaip_task_protocol_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join("changed.txt"), "before").unwrap();
        fs::write(root.join("deleted.txt"), "gone").unwrap();
        fs::write(root.join("target/ignored.txt"), "ignored").unwrap();
        let context = ProcessExecutionContext::from_execution_context(
            &crate::worker::process::ExecutionContext {
                workspace: crate::worker::process::ExecutionWorkspace {
                    root: root.to_string_lossy().into_owned(),
                    mode: crate::worker::process::WorkspaceMode::Isolated,
                },
                cwd: None,
                env: Default::default(),
                timeout_ms: None,
            },
        )
        .unwrap();
        let before = workspace_snapshot(&context);
        fs::write(root.join("changed.txt"), "after").unwrap();
        fs::write(root.join("added.txt"), "new").unwrap();
        fs::remove_file(root.join("deleted.txt")).unwrap();
        let diff = workspace_diff(&before, &context);
        assert_eq!(diff["available"], true);
        assert_eq!(diff["added"], json!(["added.txt"]));
        assert_eq!(diff["modified"], json!(["changed.txt"]));
        assert_eq!(diff["deleted"], json!(["deleted.txt"]));
        assert_eq!(diff["counts"]["added"], 1);
        assert_eq!(diff["counts"]["modified"], 1);
        assert_eq!(diff["counts"]["deleted"], 1);
        let _ = fs::remove_dir_all(&root);
    }
}
