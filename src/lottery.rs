//! 彩票业务包运行时：CSV 导入、确定性统计、滚动回测与 Pi 解释。
//!
//! 基础事实（LotteryDraw）与推导结果（AnalysisRun/Prediction）严格分离：
//! 导入和统计不依赖 Pi；Pi 只解释已保存的结果并生成新的 AnalysisRun。

use crate::cognitive::{AgentView, CognitiveEngine, PiEngine};
use crate::storage::{LibSQLStorage, ObjectEngine, ObjectRecord, StorageError, WorldCommand};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use thiserror::Error;

mod source;
use source::{CsvFileSource, LotteryDataSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LotteryGame {
    Dlt,
    Double,
}

impl LotteryGame {
    pub fn parse(value: &str) -> Result<Self, LotteryError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "dlt" | "大乐透" => Ok(Self::Dlt),
            "double" | "ssq" | "双色球" => Ok(Self::Double),
            other => Err(LotteryError::Invalid(format!(
                "不支持的彩种 {other}，可选 dlt 或 double"
            ))),
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Dlt => "dlt",
            Self::Double => "double",
        }
    }

    fn spec(self) -> GameSpec {
        match self {
            Self::Dlt => GameSpec {
                red_count: 5,
                red_max: 35,
                blue_count: 2,
                blue_max: 12,
                red_prefix: "前区",
                blue_prefix: "后区",
                date_names: &["日期"],
            },
            Self::Double => GameSpec {
                red_count: 6,
                red_max: 33,
                blue_count: 1,
                blue_max: 16,
                red_prefix: "红球",
                blue_prefix: "蓝球",
                date_names: &["开奖日期", "日期"],
            },
        }
    }

    pub fn draw_type(self) -> &'static str {
        "LotteryDraw"
    }

    pub fn import_id(self) -> String {
        format!("lottery:import:{}-csv", self.code())
    }
}

#[derive(Debug, Clone, Copy)]
struct GameSpec {
    red_count: usize,
    red_max: u8,
    blue_count: usize,
    blue_max: u8,
    red_prefix: &'static str,
    blue_prefix: &'static str,
    date_names: &'static [&'static str],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LotteryDraw {
    pub id: String,
    pub game: String,
    pub issue: String,
    pub draw_date: String,
    pub red_numbers: Vec<u8>,
    pub blue_numbers: Vec<u8>,
    pub source: String,
}

impl LotteryDraw {
    fn data(&self) -> Value {
        json!({
            "game": format!("lottery:game:{}", self.game),
            "issue": self.issue,
            "draw_date": self.draw_date,
            "red_numbers": join_numbers(&self.red_numbers),
            "blue_numbers": join_numbers(&self.blue_numbers),
            "source": self.source,
            "imported_at": crate::trace::now_ts(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportReport {
    pub game: String,
    pub source_file: String,
    pub source_hash: String,
    pub source_rows: usize,
    pub imported: usize,
    pub skipped_existing: usize,
    pub conflicts: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceInspection {
    pub row_count: usize,
    pub new_issue_count: usize,
    pub duplicate_issues: Vec<String>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportConflict {
    pub draw_id: String,
    pub issue: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
struct ImportPlan {
    new_draws: Vec<LotteryDraw>,
    skipped_existing: usize,
    conflicts: Vec<ImportConflict>,
}

struct DrawReconciler {
    existing: HashMap<String, ObjectRecord>,
}

impl DrawReconciler {
    fn new(existing: HashMap<String, ObjectRecord>) -> Self {
        Self { existing }
    }

    fn reconcile(&self, rows: Vec<LotteryDraw>) -> Result<ImportPlan, LotteryError> {
        let mut new_draws = Vec::new();
        let mut skipped_existing = 0;
        let mut conflicts = Vec::new();
        let mut seen_ids = HashSet::new();

        for draw in rows {
            if !seen_ids.insert(draw.id.clone()) {
                return Err(LotteryError::Invalid(format!(
                    "CSV 中存在重复期号：{}",
                    draw.issue
                )));
            }
            match self.existing.get(&draw.id) {
                None => new_draws.push(draw),
                Some(record) if draw_matches(record, &draw) => skipped_existing += 1,
                Some(_) => conflicts.push(ImportConflict {
                    draw_id: draw.id,
                    issue: draw.issue,
                    reason: "相同期号已有不同开奖事实，拒绝覆盖".into(),
                }),
            }
        }

        Ok(ImportPlan {
            new_draws,
            skipped_existing,
            conflicts,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatisticsReport {
    pub game: String,
    pub sample_size: usize,
    pub as_of_issue: String,
    pub window: usize,
    pub red_frequency: BTreeMap<u8, usize>,
    pub blue_frequency: BTreeMap<u8, usize>,
    pub red_candidate: Vec<u8>,
    pub blue_candidate: Vec<u8>,
    pub average_red_sum: f64,
    pub average_red_span: f64,
    pub average_consecutive_pairs: f64,
    pub average_odd_count: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestReport {
    pub game: String,
    pub target_count: usize,
    pub train_window: usize,
    pub mean_red_hits: f64,
    pub mean_blue_hits: f64,
    pub max_red_hits: usize,
    pub random_baseline_red_hits: f64,
    pub random_baseline_blue_hits: f64,
}

#[derive(Debug, Error)]
pub enum LotteryError {
    #[error("彩票业务错误：{0}")]
    Invalid(String),
    #[error("CSV 错误：{0}")]
    Csv(#[from] csv::Error),
    #[error("存储错误：{0}")]
    Storage(#[from] StorageError),
    #[error("JSON 错误：{0}")]
    Json(#[from] serde_json::Error),
    #[error("Pi Agent 错误：{0}")]
    Cognitive(String),
}

pub async fn inspect_csv(
    storage: &LibSQLStorage,
    game: LotteryGame,
    file: impl AsRef<Path>,
    _project_id: &str,
) -> Result<SourceInspection, LotteryError> {
    let source = CsvFileSource::new(file);
    let rows = source.load(game)?;
    let existing_ids: HashSet<String> = storage
        .list_all_objects()
        .await?
        .into_iter()
        .filter(|record| record.r#type == game.draw_type())
        .filter_map(|record| {
            let data_game = record
                .data
                .get("game")
                .and_then(Value::as_str)
                .unwrap_or("");
            (data_game == format!("lottery:game:{}", game.code())).then_some(record.id)
        })
        .collect();
    let mut seen = HashSet::new();
    let mut duplicate_issues = Vec::new();
    for row in &rows {
        if !seen.insert(row.issue.clone()) && !duplicate_issues.contains(&row.issue) {
            duplicate_issues.push(row.issue.clone());
        }
    }
    let new_issue_count = rows
        .iter()
        .filter(|row| !existing_ids.contains(&row.id))
        .count();
    Ok(SourceInspection {
        row_count: rows.len(),
        new_issue_count,
        duplicate_issues,
        issues: Vec::new(),
    })
}

pub async fn import_csv(
    storage: &LibSQLStorage,
    game: LotteryGame,
    file: impl AsRef<Path>,
    project_id: &str,
) -> Result<ImportReport, LotteryError> {
    ensure_project(storage, project_id).await?;
    let source = CsvFileSource::new(file);
    let rows = source.load(game)?;
    let source_file = source.source_file().to_string_lossy().into_owned();
    let source_hash = source.source_hash()?;

    let existing: HashMap<String, ObjectRecord> = storage
        .list_all_objects()
        .await?
        .into_iter()
        .filter(|record| record.r#type == game.draw_type())
        .filter_map(|record| {
            let data_game = record
                .data
                .get("game")
                .and_then(Value::as_str)
                .unwrap_or("");
            (data_game == format!("lottery:game:{}", game.code()))
                .then_some((record.id.clone(), record))
        })
        .collect();
    let plan = DrawReconciler::new(existing).reconcile(rows)?;
    let mut commands = plan
        .new_draws
        .iter()
        .map(|draw| WorldCommand::CreateObject {
            object_type: game.draw_type().into(),
            id: draw.id.clone(),
            data: draw.data(),
            project_id: Some(project_id.to_string()),
        })
        .collect::<Vec<_>>();

    let batch_id = format!("lottery:import-batch:{}:{}", game.code(), source_hash);
    let conflict_ids = plan
        .conflicts
        .iter()
        .map(|conflict| {
            format!(
                "lottery:import-conflict:{}:{}:{}",
                game.code(),
                source_hash,
                conflict.issue
            )
        })
        .collect::<Vec<_>>();
    let import_data = json!({
        "game": format!("lottery:game:{}", game.code()),
        "source_file": source_file,
        "source_hash": source_hash,
        "format": "csv",
        "row_count": plan.new_draws.len() + plan.skipped_existing + plan.conflicts.len(),
        "imported_count": plan.new_draws.len(),
        "skipped_count": plan.skipped_existing,
        "conflict_count": plan.conflicts.len(),
        "conflict_ids": conflict_ids,
        "status": if plan.conflicts.is_empty() { "已导入" } else { "已导入但存在冲突" },
    });
    if storage.get(&batch_id).await?.is_some() {
        commands.push(WorldCommand::UpdateObject {
            id: batch_id,
            data: import_data.clone(),
            project_id: Some(project_id.to_string()),
        });
    } else {
        commands.push(WorldCommand::CreateObject {
            object_type: "DataImportBatch".into(),
            id: batch_id,
            data: import_data.clone(),
            project_id: Some(project_id.to_string()),
        });
    }

    for conflict in &plan.conflicts {
        let conflict_id = format!(
            "lottery:import-conflict:{}:{}:{}",
            game.code(),
            source_hash,
            conflict.issue
        );
        let conflict_data = json!({
            "game": format!("lottery:game:{}", game.code()),
            "source_hash": source_hash,
            "draw_id": conflict.draw_id,
            "issue": conflict.issue,
            "reason": conflict.reason,
            "status": "未覆盖",
        });
        if storage.get(&conflict_id).await?.is_some() {
            commands.push(WorldCommand::UpdateObject {
                id: conflict_id,
                data: conflict_data,
                project_id: Some(project_id.to_string()),
            });
        } else {
            commands.push(WorldCommand::CreateObject {
                object_type: "DataImportConflict".into(),
                id: conflict_id,
                data: conflict_data,
                project_id: Some(project_id.to_string()),
            });
        }
    }

    let import_id = game.import_id();
    let summary_data = json!({
        "game": format!("lottery:game:{}", game.code()),
        "source_file": source_file,
        "source_hash": source_hash,
        "format": "csv",
        "row_count": plan.new_draws.len() + plan.skipped_existing + plan.conflicts.len(),
        "imported_count": plan.new_draws.len(),
        "skipped_count": plan.skipped_existing,
        "conflict_count": plan.conflicts.len(),
        "last_batch_id": format!("lottery:import-batch:{}:{}", game.code(), source_hash),
        "status": if plan.conflicts.is_empty() { "已导入" } else { "已导入但存在冲突" },
    });
    if storage.get(&import_id).await?.is_some() {
        commands.push(WorldCommand::UpdateObject {
            id: import_id,
            data: summary_data,
            project_id: Some(project_id.to_string()),
        });
    } else {
        commands.push(WorldCommand::CreateObject {
            object_type: "DataImport".into(),
            id: import_id,
            data: summary_data,
            project_id: Some(project_id.to_string()),
        });
    }
    storage.apply_world_many(&commands).await?;

    Ok(ImportReport {
        game: game.code().into(),
        source_file,
        source_hash,
        source_rows: plan.new_draws.len() + plan.skipped_existing + plan.conflicts.len(),
        imported: plan.new_draws.len(),
        skipped_existing: plan.skipped_existing,
        conflicts: plan.conflicts.len(),
    })
}

pub async fn analyze(
    storage: &LibSQLStorage,
    game: LotteryGame,
    window: usize,
    project_id: &str,
) -> Result<StatisticsReport, LotteryError> {
    ensure_project(storage, project_id).await?;
    let draws = load_draws(storage, game).await?;
    if draws.is_empty() {
        return Err(LotteryError::Invalid(format!(
            "{} 尚无 LotteryDraw 数据，请先导入 CSV",
            game.code()
        )));
    }
    let report = calculate_statistics(&draws, game.spec(), window);
    persist_statistics(storage, game, &report, project_id).await?;
    Ok(report)
}

pub async fn backtest(
    storage: &LibSQLStorage,
    game: LotteryGame,
    train_window: usize,
    test_count: usize,
    project_id: &str,
) -> Result<BacktestReport, LotteryError> {
    ensure_project(storage, project_id).await?;
    let draws = load_draws(storage, game).await?;
    let report = calculate_backtest(&draws, game.spec(), train_window, test_count)?;
    persist_backtest(storage, game, &draws, &report, project_id).await?;
    Ok(report)
}

pub async fn run_with_pi(
    storage: &LibSQLStorage,
    game: LotteryGame,
    window: usize,
    test_count: usize,
    project_id: &str,
) -> Result<Value, LotteryError> {
    let statistics = analyze(storage, game, window, project_id).await?;
    let backtest = backtest(storage, game, window, test_count, project_id).await?;
    let as_of = statistics.as_of_issue.clone();
    let stats_json = serde_json::to_value(&statistics)?;
    let backtest_json = serde_json::to_value(&backtest)?;
    let engine = PiEngine::new("lottery-agent");
    if !engine.health() {
        return Err(LotteryError::Cognitive(
            "未找到 pi 可执行文件；请安装并确保 pi 在 PATH 中，或设置 PI_BIN".into(),
        ));
    }
    let view = AgentView {
        goal: format!(
            "解释 {} 的统计分析和回测，不宣称中奖能力；给出可复核的下一步分析建议。\n统计：{}\n回测：{}",
            game.code(),
            stats_json,
            backtest_json
        ),
        world: json!({ "game": game.code(), "statistics": stats_json, "backtest": backtest_json }),
        related: Vec::new(),
        memory: Vec::new(),
        capabilities: vec!["lottery.review".into()],
        entities: Vec::new(),
        subgraph: None,
        evolution: Vec::new(),
    };
    let decision = engine
        .decide(&view)
        .map_err(|e| LotteryError::Cognitive(e.to_string()))?;
    let decision_json = match decision {
        Some(decision) => json!({
            "capability": decision.capability,
            "args": decision.args,
            "confidence": decision.confidence,
            "reasoning": "Pi Agent 已基于结构化统计与回测上下文作出解释任务决策",
        }),
        None => json!({ "capability": null, "reasoning": "Pi Agent 判定当前无需追加动作" }),
    };
    let id = format!(
        "lottery:analysis:{}:pi:w{}:n{}:{}",
        game.code(),
        window,
        test_count,
        as_of
    );
    let data = json!({
        "game": format!("lottery:game:{}", game.code()),
        "strategy": "pi-agent",
        "as_of_issue": as_of,
        "status": "completed",
        "result_summary": decision_json.to_string(),
    });
    upsert_object(storage, "AnalysisRun", &id, data, project_id).await?;
    Ok(decision_json)
}

async fn persist_statistics(
    storage: &LibSQLStorage,
    game: LotteryGame,
    report: &StatisticsReport,
    project_id: &str,
) -> Result<(), LotteryError> {
    let run_id = format!(
        "lottery:analysis:{}:statistics:{}",
        game.code(),
        report.as_of_issue
    );
    let prediction_id = format!(
        "lottery:prediction:{}:statistics:{}",
        game.code(),
        report.as_of_issue
    );
    upsert_object(
        storage,
        "AnalysisRun",
        &run_id,
        json!({
            "game": format!("lottery:game:{}", game.code()),
            "strategy": "lottery:strategy:statistics",
            "as_of_issue": report.as_of_issue,
            "status": "completed",
            "result_summary": serde_json::to_string(report)?,
        }),
        project_id,
    )
    .await?;
    upsert_object(
        storage,
        "Prediction",
        &prediction_id,
        json!({
            "game": format!("lottery:game:{}", game.code()),
            "strategy": "lottery:strategy:statistics",
            "draw_issue": next_issue(&report.as_of_issue),
            "numbers": format!("{} | {}", join_numbers(&report.red_candidate), join_numbers(&report.blue_candidate)),
            "confidence": 0.0,
            "evidence": format!("analysis_run={run_id}; status=not_predictive; historical_statistics_only"),
            "status": "candidate",
        }),
        project_id,
    )
    .await?;
    link_if_missing(storage, &run_id, "produces", &prediction_id, project_id).await?;
    Ok(())
}

async fn persist_backtest(
    storage: &LibSQLStorage,
    game: LotteryGame,
    draws: &[LotteryDraw],
    report: &BacktestReport,
    project_id: &str,
) -> Result<(), LotteryError> {
    let as_of = draws.last().map(|d| d.issue.as_str()).unwrap_or("empty");
    let run_id = format!(
        "lottery:analysis:{}:backtest:tw{}:n{}:{}",
        game.code(),
        report.train_window,
        report.target_count,
        as_of
    );
    upsert_object(
        storage,
        "AnalysisRun",
        &run_id,
        json!({
            "game": format!("lottery:game:{}", game.code()),
            "strategy": "lottery:strategy:statistics",
            "as_of_issue": as_of,
            "status": "backtested",
            "result_summary": serde_json::to_string(report)?,
        }),
        project_id,
    )
    .await?;
    let prediction_id = format!("lottery:prediction:{}:statistics:{}", game.code(), as_of);
    if storage.get(&prediction_id).await?.is_some() {
        let mut data = storage
            .get(&prediction_id)
            .await?
            .expect("对象已确认存在")
            .data;
        data["status"] = Value::String("candidate-backtested".into());
        data["evidence"] = Value::String(format!(
            "backtest_run={run_id}; metrics={}",
            serde_json::to_string(report)?
        ));
        upsert_object(storage, "Prediction", &prediction_id, data, project_id).await?;
        link_if_missing(storage, &run_id, "supports", &prediction_id, project_id).await?;
    }
    Ok(())
}

fn draw_matches(record: &ObjectRecord, draw: &LotteryDraw) -> bool {
    let data = &record.data;
    data.get("game").and_then(Value::as_str) == Some(&format!("lottery:game:{}", draw.game))
        && data.get("issue").and_then(Value::as_str) == Some(draw.issue.as_str())
        && data.get("draw_date").and_then(Value::as_str) == Some(draw.draw_date.as_str())
        && data.get("red_numbers").and_then(Value::as_str)
            == Some(join_numbers(&draw.red_numbers).as_str())
        && data.get("blue_numbers").and_then(Value::as_str)
            == Some(join_numbers(&draw.blue_numbers).as_str())
}

async fn ensure_project(storage: &LibSQLStorage, project_id: &str) -> Result<(), LotteryError> {
    if storage.project(project_id).await?.is_none() {
        return Err(LotteryError::Invalid(format!(
            "项目 {project_id} 不存在，请先安装 domain-packs/lottery-analysis"
        )));
    }
    Ok(())
}

async fn upsert_object(
    storage: &LibSQLStorage,
    object_type: &str,
    id: &str,
    data: Value,
    project_id: &str,
) -> Result<(), LotteryError> {
    let command = if storage.get(id).await?.is_some() {
        WorldCommand::UpdateObject {
            id: id.into(),
            data,
            project_id: Some(project_id.into()),
        }
    } else {
        WorldCommand::CreateObject {
            object_type: object_type.into(),
            id: id.into(),
            data,
            project_id: Some(project_id.into()),
        }
    };
    storage.apply_world(command).await?;
    Ok(())
}

async fn link_if_missing(
    storage: &LibSQLStorage,
    source: &str,
    edge: &str,
    target: &str,
    project_id: &str,
) -> Result<(), LotteryError> {
    let exists = storage
        .list_all_relations()
        .await?
        .iter()
        .any(|r| r.source == source && r.relation == edge && r.target == target);
    if !exists {
        storage
            .apply_world(WorldCommand::LinkRelation {
                source: source.into(),
                edge: edge.into(),
                target: target.into(),
                project_id: Some(project_id.into()),
            })
            .await?;
    }
    Ok(())
}

async fn load_draws(
    storage: &LibSQLStorage,
    game: LotteryGame,
) -> Result<Vec<LotteryDraw>, LotteryError> {
    let mut draws = Vec::new();
    let game_ref = format!("lottery:game:{}", game.code());
    for record in storage.list_all_objects().await? {
        if record.r#type != game.draw_type()
            || record.data.get("game").and_then(Value::as_str) != Some(game_ref.as_str())
        {
            continue;
        }
        draws.push(draw_from_object(record)?);
    }
    draws.sort_by_key(|d| issue_key(&d.issue));
    Ok(draws)
}

fn draw_from_object(record: ObjectRecord) -> Result<LotteryDraw, LotteryError> {
    let data = record.data;
    let game_ref = data.get("game").and_then(Value::as_str).unwrap_or("");
    let game = game_ref
        .strip_prefix("lottery:game:")
        .unwrap_or(game_ref)
        .to_string();
    Ok(LotteryDraw {
        id: record.id,
        game,
        issue: required_string(&data, "issue")?,
        draw_date: required_string(&data, "draw_date")?,
        red_numbers: parse_number_list(required_string(&data, "red_numbers")?)?,
        blue_numbers: parse_number_list(required_string(&data, "blue_numbers")?)?,
        source: data
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
    })
}

fn calculate_statistics(draws: &[LotteryDraw], spec: GameSpec, window: usize) -> StatisticsReport {
    let sample = &draws[draws.len().saturating_sub(window.max(1))..];
    let mut red_frequency = BTreeMap::new();
    let mut blue_frequency = BTreeMap::new();
    let mut red_sum = 0.0;
    let mut red_span = 0.0;
    let mut consecutive = 0.0;
    let mut odd = 0.0;
    for draw in sample {
        for n in &draw.red_numbers {
            *red_frequency.entry(*n).or_insert(0) += 1;
        }
        for n in &draw.blue_numbers {
            *blue_frequency.entry(*n).or_insert(0) += 1;
        }
        red_sum += draw.red_numbers.iter().map(|n| *n as f64).sum::<f64>();
        red_span += draw
            .red_numbers
            .last()
            .unwrap_or(&0)
            .saturating_sub(*draw.red_numbers.first().unwrap_or(&0)) as f64;
        consecutive += consecutive_pairs(&draw.red_numbers) as f64;
        odd += draw.red_numbers.iter().filter(|n| **n % 2 == 1).count() as f64;
    }
    let n = sample.len().max(1) as f64;
    StatisticsReport {
        game: if spec.red_max == 35 { "dlt" } else { "double" }.into(),
        sample_size: sample.len(),
        as_of_issue: draws.last().map(|d| d.issue.clone()).unwrap_or_default(),
        window: window.max(1).min(draws.len()),
        red_candidate: ranked_numbers(&red_frequency, spec.red_count),
        blue_candidate: ranked_numbers(&blue_frequency, spec.blue_count),
        red_frequency,
        blue_frequency,
        average_red_sum: red_sum / n,
        average_red_span: red_span / n,
        average_consecutive_pairs: consecutive / n,
        average_odd_count: odd / n,
    }
}

fn calculate_backtest(
    draws: &[LotteryDraw],
    spec: GameSpec,
    train_window: usize,
    test_count: usize,
) -> Result<BacktestReport, LotteryError> {
    if draws.len() <= train_window.max(1) {
        return Err(LotteryError::Invalid(format!(
            "历史数据不足：{} 条，至少需要训练窗口 {} + 1",
            draws.len(),
            train_window
        )));
    }
    let first_target = draws.len().saturating_sub(test_count.max(1));
    let first_target = first_target.max(train_window.max(1));
    let targets = &draws[first_target..];
    let mut red_hits = Vec::new();
    let mut blue_hits = Vec::new();
    for (offset, target) in targets.iter().enumerate() {
        let end = first_target + offset;
        let start = end.saturating_sub(train_window.max(1));
        let training = &draws[start..end];
        let red = frequency(training, true);
        let blue = frequency(training, false);
        let selected_red = ranked_numbers(&red, spec.red_count);
        let selected_blue = ranked_numbers(&blue, spec.blue_count);
        red_hits.push(
            selected_red
                .iter()
                .filter(|n| target.red_numbers.contains(n))
                .count(),
        );
        blue_hits.push(
            selected_blue
                .iter()
                .filter(|n| target.blue_numbers.contains(n))
                .count(),
        );
    }
    let count = targets.len().max(1) as f64;
    Ok(BacktestReport {
        game: if spec.red_max == 35 { "dlt" } else { "double" }.into(),
        target_count: targets.len(),
        train_window,
        mean_red_hits: red_hits.iter().sum::<usize>() as f64 / count,
        mean_blue_hits: blue_hits.iter().sum::<usize>() as f64 / count,
        max_red_hits: red_hits.iter().copied().max().unwrap_or(0),
        random_baseline_red_hits: (spec.red_count * spec.red_count) as f64 / spec.red_max as f64,
        random_baseline_blue_hits: (spec.blue_count * spec.blue_count) as f64
            / spec.blue_max as f64,
    })
}

fn frequency(draws: &[LotteryDraw], red: bool) -> BTreeMap<u8, usize> {
    let mut out = BTreeMap::new();
    for draw in draws {
        for n in if red {
            &draw.red_numbers
        } else {
            &draw.blue_numbers
        } {
            *out.entry(*n).or_insert(0) += 1;
        }
    }
    out
}

fn ranked_numbers(frequency: &BTreeMap<u8, usize>, count: usize) -> Vec<u8> {
    let mut values: Vec<(u8, usize)> = frequency.iter().map(|(n, f)| (*n, *f)).collect();
    values.sort_by(|(na, fa), (nb, fb)| fb.cmp(fa).then(na.cmp(nb)));
    values.into_iter().take(count).map(|(n, _)| n).collect()
}

fn consecutive_pairs(numbers: &[u8]) -> usize {
    numbers
        .windows(2)
        .filter(|pair| pair[1] == pair[0] + 1)
        .count()
}

fn parse_number_list(value: String) -> Result<Vec<u8>, LotteryError> {
    value
        .split(',')
        .map(|part| {
            part.trim()
                .parse::<u8>()
                .map_err(|_| LotteryError::Invalid(format!("号码列表无效：{value}")))
        })
        .collect()
}

fn required_string(data: &Value, key: &str) -> Result<String, LotteryError> {
    data.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| LotteryError::Invalid(format!("LotteryDraw 缺少字段 {key}")))
}

fn join_numbers(numbers: &[u8]) -> String {
    numbers
        .iter()
        .map(|n| format!("{n:02}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn issue_key(issue: &str) -> (u8, u64, String) {
    match issue.parse::<u64>() {
        Ok(value) => (0, value, String::new()),
        Err(_) => (1, 0, issue.to_string()),
    }
}

fn next_issue(issue: &str) -> String {
    issue
        .parse::<u64>()
        .map(|n| (n + 1).to_string())
        .unwrap_or_else(|_| format!("after:{issue}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dlt_csv_parser_accepts_compact_headers() {
        let csv = "期号,前区1,前区2,前区3,前区4,前区5,后区1,后区2,日期\n1,01,02,03,04,05,01,02,2026-01-01\n";
        let rows = source::CsvParser::new(LotteryGame::Dlt)
            .parse_str(csv)
            .expect("headers");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].issue, "1");
        assert_eq!(rows[0].red_numbers, vec![1, 2, 3, 4, 5]);
        assert_eq!(rows[0].blue_numbers, vec![1, 2]);
    }

    #[tokio::test]
    async fn import_analyze_and_backtest_round_trip() {
        let storage = LibSQLStorage::open(":memory:").await.expect("db");
        storage
            .create_project(
                "lottery-analysis",
                "彩票",
                "测试",
                "object Lottery:\n  properties:\n    source: string",
                None,
            )
            .await
            .expect("project");
        let path = std::env::temp_dir().join(format!("lottery_test_{}.csv", std::process::id()));
        std::fs::write(
            &path,
            "期号,前区1,前区2,前区3,前区4,前区5,后区1,后区2,日期\n1,01,02,03,04,05,01,02,2026-01-01\n2,01,02,06,07,08,01,03,2026-01-03\n3,02,04,06,08,10,02,04,2026-01-05\n",
        )
        .expect("csv");
        let imported = import_csv(&storage, LotteryGame::Dlt, &path, "lottery-analysis")
            .await
            .expect("import");
        assert_eq!(imported.imported, 3);
        let stats = analyze(&storage, LotteryGame::Dlt, 2, "lottery-analysis")
            .await
            .expect("analyze");
        assert_eq!(stats.sample_size, 2);
        let backtest = backtest(&storage, LotteryGame::Dlt, 2, 1, "lottery-analysis")
            .await
            .expect("backtest");
        assert_eq!(backtest.target_count, 1);
        let repeated = import_csv(&storage, LotteryGame::Dlt, &path, "lottery-analysis")
            .await
            .expect("repeat import");
        assert_eq!(repeated.imported, 0);
        assert_eq!(repeated.skipped_existing, 3);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn import_sync_records_hash_batch_and_conflict_without_overwrite() {
        let storage = LibSQLStorage::open(":memory:").await.expect("db");
        storage
            .create_project(
                "lottery-analysis",
                "彩票",
                "冲突测试",
                "object Lottery:\n  properties:\n    source: string",
                None,
            )
            .await
            .expect("project");
        let path = std::env::temp_dir().join(format!(
            "lottery_conflict_{}_{}.csv",
            std::process::id(),
            crate::trace::now_ts()
        ));
        std::fs::write(
            &path,
            "期号,前区1,前区2,前区3,前区4,前区5,后区1,后区2,日期\n1,01,02,03,04,05,01,02,2026-01-01\n",
        )
        .expect("csv");
        let first = import_csv(&storage, LotteryGame::Dlt, &path, "lottery-analysis")
            .await
            .expect("first import");
        assert_eq!(first.imported, 1);
        assert_eq!(first.conflicts, 0);
        assert_eq!(first.source_hash.len(), 64);

        std::fs::write(
            &path,
            "期号,前区1,前区2,前区3,前区4,前区5,后区1,后区2,日期\n1,01,02,03,04,06,01,02,2026-01-01\n",
        )
        .expect("conflicting csv");
        let second = import_csv(&storage, LotteryGame::Dlt, &path, "lottery-analysis")
            .await
            .expect("conflict import");
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped_existing, 0);
        assert_eq!(second.conflicts, 1);
        assert_ne!(first.source_hash, second.source_hash);

        let objects = storage.list_all_objects().await.expect("objects");
        assert!(
            objects
                .iter()
                .any(|record| record.r#type == "DataImportBatch")
        );
        assert!(
            objects
                .iter()
                .any(|record| record.r#type == "DataImportConflict")
        );
        let draw = objects
            .iter()
            .find(|record| record.id == "lottery:draw:dlt:1")
            .expect("draw");
        assert_eq!(draw.data["red_numbers"], "01,02,03,04,05");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn statistics_are_deterministic() {
        let draws = vec![
            LotteryDraw {
                id: "1".into(),
                game: "dlt".into(),
                issue: "1".into(),
                draw_date: "".into(),
                red_numbers: vec![1, 2, 3, 4, 5],
                blue_numbers: vec![1, 2],
                source: "".into(),
            },
            LotteryDraw {
                id: "2".into(),
                game: "dlt".into(),
                issue: "2".into(),
                draw_date: "".into(),
                red_numbers: vec![1, 2, 6, 7, 8],
                blue_numbers: vec![1, 3],
                source: "".into(),
            },
        ];
        let report = calculate_statistics(&draws, LotteryGame::Dlt.spec(), 2);
        assert_eq!(report.red_candidate, vec![1, 2, 3, 4, 5]);
        assert_eq!(report.average_consecutive_pairs, 3.5);
    }
}
