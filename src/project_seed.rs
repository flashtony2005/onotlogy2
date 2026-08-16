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
# 关系：Project --contains--> Module --contains--> File
#       Module --depends_on--> Module（跨 crate 依赖，后续由依赖图驱动）
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
    storage
        .apply_world_many(&cmds)
        .await
        .map_err(|e| e.to_string())?;
    Ok(cmds.len())
}
