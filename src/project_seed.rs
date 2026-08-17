//! 第一个项目：OpenAIP-OS 工程本体（Phase 8 · 本体论管理本工程）。
//!
//! 项目的世界 = 工程事实：Module（crates/src 模块）/ File（源文件）/
//! Test（测试文件）对象 + contains 关系——全部作为 **事件** 写入
//! `project_id="openaip-os"` 的世界（P5 单向流：Command → Event → Projection →
//! World State），世界查询按项目过滤即得工程结构。
//!
//! 事情（Task）维度：本体论方案——项目世界里的 Task 对象 + 关系
//!（Project --contains--> Task），不引入第二套表。

use std::path::Path;

use crate::storage::{LibSQLStorage, WorldCommand};

/// OpenAIP-OS 工程本体（ODL 文本；决定项目世界语义——语义是对象的 type/关系）。
pub const OPENAI_OS_ONTOLOGY: &str = r#"
object Module:       # 工程模块（crates/ 或 src/ 顶级）
  properties: name, path
object File:         # 源文件
  properties: path, kind
object Test:         # 测试文件/用例
  properties: name, kind
object Task:         # 事情（项目下的任务/事项）
  properties: title, status
object CheckRun:     # 工程检查（make check / cargo test 门禁）
  properties: kind, status, passed, total, failed, duration_ms
object Commit:       # 工程提交（git）
  properties: hash, message, author, time
# 关系：Project --contains--> Module --contains--> File
#       Module --depends_on--> Module（跨 crate 依赖，后续由依赖图驱动）
#       root --contains--> CheckRun / Commit（工程健康度时间线）
"#;

/// 项目 id（OpenAIP-OS 工程）。
pub const OPENAI_OS_PROJECT: &str = "openaip-os";

/// seed：把当前工程事实写成事件（幂等——同 id 对象事件重复应用无副作用，
/// 投影 P2 按 id 去重；重复 seed 产生重复事件但世界状态收敛）。
pub async fn seed_openaip_os(storage: &LibSQLStorage, root: &str) -> Result<usize, String> {
    // 1) 项目注册（幂等：已存在则跳过）
    if storage
        .project(OPENAI_OS_PROJECT)
        .await
        .map_err(|e| e.to_string())?
        .is_none()
    {
        storage
            .create_project(
                OPENAI_OS_PROJECT,
                "OpenAIP-OS 工程",
                "第一个项目：用本体论方案管理本工程（模块/文件/测试 → 项目世界）",
                OPENAI_OS_ONTOLOGY,
            )
            .await
            .map_err(|e| e.to_string())?;
    }

    let mut cmds: Vec<WorldCommand> = Vec::new();
    // 2) crates/ 模块
    let crates_dir = Path::new(root).join("crates");
    if crates_dir.is_dir() {
        let mut dirs: Vec<String> = std::fs::read_dir(&crates_dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .collect();
        dirs.sort();
        for name in dirs {
            cmds.push(WorldCommand::CreateObject {
                object_type: "Module".into(),
                id: format!("module:{name}"),
                data: serde_json::json!({ "name": name, "path": format!("crates/{name}") }),
                project_id: Some(OPENAI_OS_PROJECT.into()),
            });
            // module contains 关键文件（lib.rs / Cargo.toml）
            for f in ["Cargo.toml", "src/lib.rs", "src/main.rs"] {
                let fp = crates_dir.join(&name).join(f);
                if fp.is_file() {
                    let fid = format!("file:{name}:{f}");
                    cmds.push(WorldCommand::CreateObject {
                        object_type: "File".into(),
                        id: fid.clone(),
                        data: serde_json::json!({ "path": format!("crates/{name}/{f}"), "kind": "source" }),
                        project_id: Some(OPENAI_OS_PROJECT.into()),
                    });
                    cmds.push(WorldCommand::LinkRelation {
                        source: format!("module:{name}"),
                        edge: "contains".into(),
                        target: fid,
                        project_id: Some(OPENAI_OS_PROJECT.into()),
                    });
                }
            }
        }
    }
    // 3) src/ 顶级模块（compose/cognitive/fabric/facade/projection/...）
    let src_dir = Path::new(root).join("src");
    if src_dir.is_dir() {
        let mut files: Vec<String> = std::fs::read_dir(&src_dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .filter(|f| f.ends_with(".rs"))
            .collect();
        files.sort();
        for f in files {
            cmds.push(WorldCommand::CreateObject {
                object_type: "File".into(),
                id: format!("file:src:{f}"),
                data: serde_json::json!({ "path": format!("src/{f}"), "kind": "source" }),
                project_id: Some(OPENAI_OS_PROJECT.into()),
            });
            cmds.push(WorldCommand::LinkRelation {
                source: "module:root".to_string(),
                edge: "contains".into(),
                target: format!("file:src:{f}"),
                project_id: Some(OPENAI_OS_PROJECT.into()),
            });
        }
        cmds.push(WorldCommand::CreateObject {
            object_type: "Module".into(),
            id: "module:root".into(),
            data: serde_json::json!({ "name": "root", "path": "src" }),
            project_id: Some(OPENAI_OS_PROJECT.into()),
        });
    }
    // 4) tests/ 测试文件（Test 对象）
    let tests_dir = Path::new(root).join("tests");
    if tests_dir.is_dir() {
        let mut files: Vec<String> = std::fs::read_dir(&tests_dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .filter(|f| f.ends_with(".rs"))
            .collect();
        files.sort();
        for f in files {
            cmds.push(WorldCommand::CreateObject {
                object_type: "Test".into(),
                id: format!("test:{f}"),
                data: serde_json::json!({ "name": f, "kind": "integration" }),
                project_id: Some(OPENAI_OS_PROJECT.into()),
            });
            cmds.push(WorldCommand::LinkRelation {
                source: "module:root".into(),
                edge: "contains".into(),
                target: format!("test:{f}"),
                project_id: Some(OPENAI_OS_PROJECT.into()),
            });
        }
    }
    // 幂等写入：已存在的对象/关系跳过（重复 seed / check / commit 安全）
    apply_idempotent(storage, &cmds).await?;
    Ok(cmds.len())
}

/// 幂等 apply：已存在对象（UNIQUE）或重复关系边（WB2007）跳过；
/// 其余错误透出。seed/check/commit 复用——世界收敛，不重复爆错。
async fn apply_idempotent(storage: &LibSQLStorage, cmds: &[WorldCommand]) -> Result<(), String> {
    for cmd in cmds {
        match storage.apply_world(cmd.clone()).await {
            Ok(()) => {}
            Err(e) => {
                let text = e.to_string();
                let idempotent =
                    text.contains("UNIQUE constraint failed") || text.contains("WB2007");
                if !idempotent {
                    return Err(text);
                }
            }
        }
    }
    Ok(())
}

/// 记录一次工程检查（make check / cargo test 门禁结果）→ 项目世界 CheckRun 对象。
/// 每次检查一个新对象（id=时间戳）——世界保留健康度时间线。
pub async fn record_check_run(
    storage: &LibSQLStorage,
    status: &str,
    passed: usize,
    failed: usize,
    total: usize,
    duration_ms: u64,
) -> Result<(), String> {
    let id = format!("check:{}", crate::trace::now_ts());
    let cmds = vec![
        WorldCommand::CreateObject {
            object_type: "CheckRun".into(),
            id: id.clone(),
            data: serde_json::json!({
                "kind": "make_check",
                "status": status,
                "passed": passed,
                "failed": failed,
                "total": total,
                "duration_ms": duration_ms,
            }),
            project_id: Some(OPENAI_OS_PROJECT.into()),
        },
        WorldCommand::LinkRelation {
            source: "module:root".into(),
            edge: "contains".into(),
            target: id,
            project_id: Some(OPENAI_OS_PROJECT.into()),
        },
    ];
    apply_idempotent(storage, &cmds).await
}

/// 记录一次工程提交（git）→ 项目世界 Commit 对象（含 message/hash）。
pub async fn record_commit(
    storage: &LibSQLStorage,
    hash: &str,
    message: &str,
    author: &str,
    time: &str,
) -> Result<(), String> {
    let id = format!("commit:{hash}");
    let cmds = vec![
        WorldCommand::CreateObject {
            object_type: "Commit".into(),
            id: id.clone(),
            data: serde_json::json!({
                "hash": hash,
                "message": message,
                "author": author,
                "time": time,
            }),
            project_id: Some(OPENAI_OS_PROJECT.into()),
        },
        WorldCommand::LinkRelation {
            source: "module:root".into(),
            edge: "contains".into(),
            target: id,
            project_id: Some(OPENAI_OS_PROJECT.into()),
        },
    ];
    apply_idempotent(storage, &cmds).await
}

/// 依赖图 seed：扫描 crates/*/Cargo.toml 的 workspace 内依赖（oaip-*）
/// → `module:xxx --depends_on--> module:yyy` 事件（project=openaip-os）。
/// 幂等：重复边（WB2007）跳过。
pub async fn seed_dependencies(storage: &LibSQLStorage, root: &str) -> Result<usize, String> {
    let crates_dir = std::path::Path::new(root).join("crates");
    if !crates_dir.is_dir() {
        return Ok(0);
    }
    let mut cmds: Vec<WorldCommand> = Vec::new();
    let mut dirs: Vec<String> = std::fs::read_dir(&crates_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .collect();
    dirs.sort();
    for name in &dirs {
        let cargo = crates_dir.join(name).join("Cargo.toml");
        let Ok(text) = std::fs::read_to_string(&cargo) else {
            continue;
        };
        // [dependencies] 段内的 oaip-* 引用（workspace 内部 crate）
        let in_deps = text
            .lines()
            .skip_while(|l| l.trim() != "[dependencies]")
            .skip(1)
            .take_while(|l| !l.trim_start().starts_with('['))
            .collect::<Vec<_>>()
            .join(
                "
",
            );
        for dep in in_deps
            .lines()
            .filter_map(|l| l.split_whitespace().next())
            .filter(|t| t.starts_with("oaip-"))
            .map(|t| t.trim_matches(|c| c == '"' || c == ','))
        {
            if !dirs.iter().any(|d| d == dep) {
                continue; // 只连 workspace 内模块
            }
            cmds.push(WorldCommand::LinkRelation {
                source: format!("module:{name}"),
                edge: "depends_on".into(),
                target: format!("module:{dep}"),
                project_id: Some(OPENAI_OS_PROJECT.into()),
            });
        }
    }
    apply_idempotent(storage, &cmds).await?;
    Ok(cmds.len())
}

/// 给项目世界加一个对象（事情/Issue/任意本体类型）：
/// CreateObject(project) + 可选 root --contains--> 关系（--edge 给定时）。
/// 幂等：同 id 已存在则跳过（世界收敛）。
pub async fn project_add_object(
    storage: &LibSQLStorage,
    object_type: &str,
    id: &str,
    project_id: &str,
    data: &serde_json::Value,
    edge: Option<&str>,
) -> Result<(), String> {
    let mut cmds = vec![WorldCommand::CreateObject {
        object_type: object_type.to_string(),
        id: id.to_string(),
        data: data.clone(),
        project_id: Some(project_id.to_string()),
    }];
    if let Some(e) = edge {
        cmds.push(WorldCommand::LinkRelation {
            source: "module:root".to_string(),
            edge: e.to_string(),
            target: id.to_string(),
            project_id: Some(project_id.to_string()),
        });
    }
    apply_idempotent(storage, &cmds).await
}
