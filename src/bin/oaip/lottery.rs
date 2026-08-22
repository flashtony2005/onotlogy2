use openaip_os::LibSQLStorage;
use openaip_os::lottery::{BacktestReport, ImportReport, StatisticsReport};
use openaip_os::lottery_plugin;
use openaip_os::plugin::catalog::PluginCatalog;
use serde_json::json;
use std::path::Path;

pub async fn cmd_import(
    db: &str,
    game: &str,
    file: &str,
    project: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let source_root = Path::new(file)
        .parent()
        .map(Path::to_path_buf)
        .ok_or("彩票 CSV 路径必须包含父目录")?;
    let catalog = PluginCatalog::with_builtins(storage, [source_root])?;
    let report: ImportReport = serde_json::from_value(
        catalog
            .invoke(
                lottery_plugin::PLUGIN_ID,
                lottery_plugin::SYNC_CAPABILITY,
                json!({ "game": game, "file": file, "project": project }),
            )
            .await?,
    )?;
    println!(
        "✅ 彩票 CSV 导入完成：game={} source={} rows={} imported={} skipped={}",
        report.game,
        report.source_file,
        report.source_rows,
        report.imported,
        report.skipped_existing
    );
    Ok(())
}

pub async fn cmd_analyze(
    db: &str,
    game: &str,
    window: usize,
    project: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let catalog = PluginCatalog::with_builtins(storage, std::iter::empty())?;
    let report: StatisticsReport = serde_json::from_value(
        catalog
            .invoke(
                lottery_plugin::PLUGIN_ID,
                lottery_plugin::ANALYZE_CAPABILITY,
                json!({ "game": game, "window": window, "project": project }),
            )
            .await?,
    )?;
    println!(
        "📊 彩票统计分析完成：{}",
        serde_json::to_string_pretty(&report)?
    );
    Ok(())
}

pub async fn cmd_backtest(
    db: &str,
    game: &str,
    train_window: usize,
    test_count: usize,
    project: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let catalog = PluginCatalog::with_builtins(storage, std::iter::empty())?;
    let report: BacktestReport = serde_json::from_value(
        catalog
            .invoke(
                lottery_plugin::PLUGIN_ID,
                lottery_plugin::BACKTEST_CAPABILITY,
                json!({
                    "game": game,
                    "train_window": train_window,
                    "test_count": test_count,
                    "project": project
                }),
            )
            .await?,
    )?;
    println!(
        "🧪 彩票滚动回测完成：{}",
        serde_json::to_string_pretty(&report)?
    );
    Ok(())
}

pub async fn cmd_run(
    db: &str,
    game: &str,
    window: usize,
    test_count: usize,
    project: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let catalog = PluginCatalog::with_builtins(storage, std::iter::empty())?;
    let statistics: StatisticsReport = serde_json::from_value(
        catalog
            .invoke(
                lottery_plugin::PLUGIN_ID,
                lottery_plugin::ANALYZE_CAPABILITY,
                json!({ "game": game, "window": window, "project": project }),
            )
            .await?,
    )?;
    let backtest: BacktestReport = serde_json::from_value(
        catalog
            .invoke(
                lottery_plugin::PLUGIN_ID,
                lottery_plugin::BACKTEST_CAPABILITY,
                json!({ "game": game, "train_window": window, "test_count": test_count, "project": project }),
            )
            .await?,
    )?;
    println!("📊 统计：{}", serde_json::to_string_pretty(&statistics)?);
    println!("🧪 回测：{}", serde_json::to_string_pretty(&backtest)?);
    Ok(())
}

pub async fn cmd_pi(
    db: &str,
    game: &str,
    window: usize,
    test_count: usize,
    project: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let catalog = PluginCatalog::with_builtins(storage, std::iter::empty())?;
    let result = catalog
        .invoke(
            lottery_plugin::PLUGIN_ID,
            lottery_plugin::REVIEW_CAPABILITY,
            json!({ "game": game, "window": window, "test_count": test_count, "project": project }),
        )
        .await?;
    println!(
        "🤖 Pi Agent 彩票解释完成：{}",
        serde_json::to_string_pretty(&result)?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn cli_lottery_commands_use_shared_plugin_catalog() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let db = std::env::temp_dir().join(format!("oaip_cli_lottery_{suffix}.db"));
        let csv = std::env::temp_dir().join(format!("oaip_cli_lottery_{suffix}.csv"));
        let db = db.to_string_lossy().into_owned();
        let csv = csv.to_string_lossy().into_owned();

        let storage = LibSQLStorage::open(&db).await.expect("db");
        storage
            .create_project("lottery-analysis", "彩票", "CLI 测试", "ontology")
            .await
            .expect("project");
        std::fs::write(
            &csv,
            "期号,前区1,前区2,前区3,前区4,前区5,后区1,后区2,日期\n1,01,02,03,04,05,01,02,2026-01-01\n2,01,02,06,07,08,01,03,2026-01-03\n3,02,04,06,08,10,02,04,2026-01-05\n",
        )
        .expect("csv");
        drop(storage);

        cmd_import(&db, "dlt", &csv, "lottery-analysis")
            .await
            .expect("import");
        cmd_analyze(&db, "dlt", 2, "lottery-analysis")
            .await
            .expect("analyze");
        cmd_backtest(&db, "dlt", 2, 1, "lottery-analysis")
            .await
            .expect("backtest");
        cmd_run(&db, "dlt", 2, 1, "lottery-analysis")
            .await
            .expect("run");

        let _ = std::fs::remove_file(&csv);
        let _ = std::fs::remove_file(&db);
    }
}
