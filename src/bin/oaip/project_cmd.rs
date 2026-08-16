//! Project 命令（Phase 8 遗留边界闭环）：工程健康度 → 项目世界。
//!
//! - `oaip facade project check`：运行 make check（完整门禁）+ 汇总测试数 →
//!   写 CheckRun 对象到 openaip-os 项目世界（状态时间线）。
//! - `oaip facade project commit`：读最近 git 提交 → 写 Commit 对象到项目世界。

use std::sync::Arc;

use openaip_os::project_seed::{record_check_run, record_commit, seed_openaip_os};
use openaip_os::storage::LibSQLStorage;

fn open(
    db: &str,
    handle: &tokio::runtime::Handle,
) -> Result<Arc<LibSQLStorage>, Box<dyn std::error::Error>> {
    Ok(Arc::new(handle.block_on(LibSQLStorage::open(db))?))
}

/// 跑 make check（完整门禁）+ 汇总测试结果 → CheckRun 事件 → 项目世界。
pub fn cmd_project_check(db: &str) -> Result<(), Box<dyn std::error::Error>> {
    let handle = tokio::runtime::Handle::current();
    let storage = open(db, &handle)?;
    // 确保项目与工程结构在（幂等）
    handle.block_on(seed_openaip_os(&storage, "."))?;

    let start = std::time::Instant::now();
    let out = std::process::Command::new("make")
        .arg("check")
        .output()
        .map_err(|e| format!("make check 启动失败: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let combined = format!("{stdout}\n{stderr}");
    let duration_ms = start.elapsed().as_millis() as u64;

    // 汇总 test result: ok. N passed; M failed
    let mut passed = 0usize;
    let mut failed = 0usize;
    for line in combined.lines() {
        let line = line.trim();
        if !line.starts_with("test result:") {
            continue;
        }
        for seg in line.split(';') {
            let seg = seg.trim();
            if let Some(rest) = seg.strip_suffix(" passed")
                && let Ok(n) = rest
                    .split_whitespace()
                    .last()
                    .unwrap_or("")
                    .parse::<usize>()
            {
                passed += n;
            } else if let Some(rest) = seg.strip_suffix(" failed")
                && let Ok(n) = rest
                    .split_whitespace()
                    .last()
                    .unwrap_or("")
                    .parse::<usize>()
            {
                failed += n;
            }
        }
    }
    let total = passed + failed;
    let ok = out.status.success();
    let status = if ok { "passed" } else { "failed" };

    handle.block_on(record_check_run(
        &storage,
        status,
        passed,
        failed,
        total,
        duration_ms,
    ))?;
    println!(
        "✅ make check {}：{} passed / {} failed（{} ms）→ CheckRun 入项目世界",
        if ok { "通过" } else { "失败" },
        passed,
        failed,
        duration_ms
    );
    if !ok {
        // 输出门禁摘要（失败原因可见）
        let summary: Vec<&str> = combined
            .lines()
            .filter(|l| l.contains("error") || l.contains("FAILED") || l.contains("panicked"))
            .take(8)
            .collect();
        for l in summary {
            println!("   {l}");
        }
    }
    Ok(())
}

/// 读最近 git 提交 → Commit 事件 → 项目世界。
pub fn cmd_project_commit(db: &str) -> Result<(), Box<dyn std::error::Error>> {
    let handle = tokio::runtime::Handle::current();
    let storage = open(db, &handle)?;
    handle.block_on(seed_openaip_os(&storage, "."))?;

    let hash = run_git(&["log", "-1", "--format=%H"])?;
    let message = run_git(&["log", "-1", "--format=%s"])?;
    let author = run_git(&["log", "-1", "--format=%an"])?;
    let time = run_git(&["log", "-1", "--format=%cI"])?;
    handle.block_on(record_commit(&storage, &hash, &message, &author, &time))?;
    println!("📌 Commit 入项目世界：{hash} {message}（{author} {time}）");
    Ok(())
}

fn run_git(args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let out = std::process::Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("git 执行失败: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
