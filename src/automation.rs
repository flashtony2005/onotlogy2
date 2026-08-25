//! Phase 2.5 自动运行引擎：watcher、持久化队列和业务包调度。
//!
//! Watcher 只负责发现数据源变化；Scheduler 只负责领取任务并调用插件。
//! 这样业务包能力仍由 PluginCatalog 提供，自动化层不复制彩票领域实现。

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::domain_pack::DomainPack;
use crate::domain_pack::DomainPackRuntime;
use crate::plugin::catalog::SharedPluginCatalog;
use crate::storage::{AutomationJob, LibSQLStorage, ObjectEngine, TraceQuery, WorldCommand};

const SYNC: &str = "sync";
const IMPORT: &str = "import";
const ANALYZE: &str = "analyze";
const BACKTEST: &str = "backtest";
const REVIEW: &str = "review";
const DEFAULT_WINDOW: u64 = 50;
const DEFAULT_TEST_COUNT: u64 = 30;

/// 业务包调度器的稳定操作协议；存储层只保存其字符串表示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainPackOperation {
    Sync,
    Import,
    Analyze,
    Backtest,
    Review,
}

impl DomainPackOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sync => SYNC,
            Self::Import => IMPORT,
            Self::Analyze => ANALYZE,
            Self::Backtest => BACKTEST,
            Self::Review => REVIEW,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            SYNC => Some(Self::Sync),
            IMPORT => Some(Self::Import),
            ANALYZE => Some(Self::Analyze),
            BACKTEST => Some(Self::Backtest),
            REVIEW => Some(Self::Review),
            _ => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum AutomationError {
    #[error("自动运行引擎存储失败：{0}")]
    Storage(#[from] crate::storage::StorageError),
    #[error("自动运行引擎 JSON 失败：{0}")]
    Json(#[from] serde_json::Error),
    #[error("自动运行引擎失败：{0}")]
    Runtime(String),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AutomationScanError {
    pub code: String,
    pub message: String,
    pub source_path: String,
    pub timestamp: String,
    pub trace_id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AutomationScanHistory {
    pub status: String,
    pub event_type: String,
    pub code: Option<String>,
    pub message: Option<String>,
    pub fingerprint: Option<String>,
    pub source_path: Option<String>,
    pub timestamp: String,
    pub trace_id: String,
    pub job_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AutomationScanSummary {
    pub source_rows: Option<u64>,
    pub new_issue_count: Option<u64>,
    pub duplicate_issues: Option<u64>,
    pub fingerprint: Option<String>,
    pub timestamp: String,
    pub trace_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AutomationManualRetry {
    pub actor: String,
    pub retry_number: u64,
    pub status: String,
    pub submitted_jobs: u64,
    pub timestamp: String,
    pub trace_id: String,
}

#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct AutomationQuality {
    pub total_jobs: u64,
    pub succeeded_jobs: u64,
    pub failed_jobs: u64,
    pub queued_jobs: u64,
    pub running_jobs: u64,
    pub cancelled_jobs: u64,
    pub retry_jobs: u64,
    pub success_rate: f64,
    pub last_review_status: Option<String>,
    pub last_review_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AutomationPackStatus {
    pub pack_id: String,
    pub runtime: DomainPackRuntime,
    pub fingerprint: Option<String>,
    pub last_success_at: Option<String>,
    /// 最近一次失败记录；即使已经恢复也保留，用于审计追溯。
    pub last_scan_error: Option<AutomationScanError>,
    /// 当前仍然生效的扫描错误；恢复后为 None，避免前端把历史错误误报为当前故障。
    pub active_scan_error: Option<AutomationScanError>,
    pub scan_state: String,
    pub scan_history: Vec<AutomationScanHistory>,
    pub last_scan_summary: Option<AutomationScanSummary>,
    pub quality: AutomationQuality,
    pub manual_retry_count: u64,
    pub last_manual_retry: Option<AutomationManualRetry>,
}

fn fingerprint_key(pack_id: &str) -> String {
    format!("automation:fingerprint:{pack_id}")
}

fn last_success_key(pack_id: &str) -> String {
    format!("automation:last_success_at:{pack_id}")
}

pub struct AutomationEngine {
    storage: Arc<LibSQLStorage>,
    plugin_catalog: SharedPluginCatalog,
    domain_packs_dir: PathBuf,
    event_tx: tokio::sync::broadcast::Sender<crate::trace::TraceEvent>,
    stop_tx: watch::Sender<bool>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl AutomationEngine {
    pub fn new(
        storage: Arc<LibSQLStorage>,
        plugin_catalog: SharedPluginCatalog,
        domain_packs_dir: PathBuf,
        event_tx: tokio::sync::broadcast::Sender<crate::trace::TraceEvent>,
    ) -> Self {
        let (stop_tx, _) = watch::channel(false);
        Self {
            storage,
            plugin_catalog,
            domain_packs_dir,
            event_tx,
            stop_tx,
            handle: Mutex::new(None),
        }
    }

    /// 启动单 watcher + 单 scheduler。重复调用不会创建第二个后台 worker。
    pub async fn start(self: &Arc<Self>) -> Result<(), AutomationError> {
        self.storage.recover_automation_jobs().await?;
        let mut guard = self
            .handle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if guard.is_some() {
            return Ok(());
        }
        let engine = Arc::clone(self);
        let mut stop_rx = self.stop_tx.subscribe();
        *guard = Some(tokio::spawn(async move {
            engine.run_loop(&mut stop_rx).await;
        }));
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.handle
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_some()
    }

    pub async fn shutdown(&self) {
        let _ = self.stop_tx.send(true);
        let handle = {
            let mut guard = self
                .handle
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            guard.take()
        };
        if let Some(handle) = handle {
            let _ = handle.await;
        }
    }

    async fn run_loop(&self, stop_rx: &mut watch::Receiver<bool>) {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        let mut last_scan: HashMap<String, SystemTime> = HashMap::new();
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if let Err(error) = self.scan_due(&mut last_scan).await {
                        eprintln!("自动 watcher 扫描失败：{error}");
                    }
                    if let Err(error) = self.run_one().await {
                        eprintln!("自动任务执行失败：{error}");
                    }
                }
                changed = stop_rx.changed() => {
                    if changed.is_err() || *stop_rx.borrow() { break; }
                }
            }
        }
    }

    async fn scan_due(
        &self,
        last_scan: &mut HashMap<String, SystemTime>,
    ) -> Result<(), AutomationError> {
        for pack_dir in pack_directories(&self.domain_packs_dir) {
            let Ok(pack) = DomainPack::load(&pack_dir) else {
                continue;
            };
            let runtime = self.runtime_for_pack(&pack.manifest.id).await?;
            if !runtime.enabled
                || !runtime.watch_enabled
                || !runtime.auto_sync
                || runtime.source_path.trim().is_empty()
            {
                continue;
            }
            let due = last_scan
                .get(&runtime.pack_id)
                .map(|at| {
                    at.elapsed().unwrap_or_default().as_secs() >= runtime.poll_interval_seconds
                })
                .unwrap_or(true);
            if due {
                match self.scan_pack_internal(&runtime).await {
                    Ok(()) => {
                        last_scan.insert(runtime.pack_id, SystemTime::now());
                    }
                    Err(error) => {
                        self.emit_scan_error(&runtime, &error).await;
                    }
                }
            }
        }
        Ok(())
    }

    /// 手动扫描一个业务包；不要求 watch_enabled，但仍尊重 enabled 和 source_path。
    pub async fn scan_pack(&self, pack_id: &str) -> Result<Vec<AutomationJob>, AutomationError> {
        self.scan_pack_by(pack_id, None).await
    }

    /// 记录网页端或桌面端发起的人工扫描，并把操作人写入审计事件。
    pub async fn scan_pack_by(
        &self,
        pack_id: &str,
        actor: Option<&str>,
    ) -> Result<Vec<AutomationJob>, AutomationError> {
        let runtime = self.runtime_for_pack(pack_id).await?;
        if !runtime.enabled {
            return Err(AutomationError::Runtime(format!("业务包 {pack_id} 未启用")));
        }
        let before = self.storage.list_automation_jobs(Some(pack_id), 20).await?;
        let retry_number = if actor.is_some() {
            self.manual_retry_count(pack_id).await? + 1
        } else {
            0
        };
        if let Err(error) = self.scan_pack_internal(&runtime).await {
            self.emit_scan_error(&runtime, &error).await;
            if let Some(actor) = actor {
                let message = error.to_string();
                self.emit_manual_retry(&runtime, actor, retry_number, 0, "failed", Some(&message))
                    .await;
            }
            return Err(error);
        }
        let after = self.storage.list_automation_jobs(Some(pack_id), 20).await?;
        let before_ids: std::collections::HashSet<_> =
            before.into_iter().map(|job| job.id).collect();
        let jobs = after
            .into_iter()
            .filter(|job| !before_ids.contains(&job.id))
            .collect::<Vec<_>>();
        if let Some(actor) = actor {
            self.emit_manual_retry(
                &runtime,
                actor,
                retry_number,
                jobs.len() as u64,
                "succeeded",
                None,
            )
            .await;
        }
        Ok(jobs)
    }

    pub async fn pack_status(
        &self,
        pack_id: &str,
    ) -> Result<AutomationPackStatus, AutomationError> {
        let runtime = self.runtime_for_pack(pack_id).await?;
        let fingerprint = self.storage.get_meta(&fingerprint_key(pack_id)).await?;
        let last_success_at = self.storage.get_meta(&last_success_key(pack_id)).await?;
        let last_scan_error = self
            .latest_scan_error(pack_id, &runtime.source_path)
            .await?;
        let scan_history = self.scan_history(pack_id, &runtime.source_path).await?;
        let last_scan_summary = self.latest_scan_summary(pack_id, &scan_history).await?;
        let jobs = self
            .storage
            .list_automation_jobs(Some(pack_id), 200)
            .await?;
        let quality = automation_quality(&jobs);
        let (manual_retry_count, last_manual_retry) = self.manual_retry_info(pack_id).await?;
        let scan_state = scan_state(last_success_at.as_deref(), last_scan_error.as_ref());
        let active_scan_error = (scan_state == "failed")
            .then(|| last_scan_error.clone())
            .flatten();
        Ok(AutomationPackStatus {
            pack_id: pack_id.to_string(),
            runtime,
            fingerprint,
            last_success_at,
            last_scan_error,
            active_scan_error,
            scan_state,
            scan_history,
            last_scan_summary,
            quality,
            manual_retry_count,
            last_manual_retry,
        })
    }

    /// 手动触发默认同步链；用于 API 和桌面端的“立即运行”。
    pub async fn run_pack(&self, pack_id: &str) -> Result<AutomationJob, AutomationError> {
        self.run_operation(pack_id, DomainPackOperation::Sync).await
    }

    /// 按统一业务包操作协议入队。所有操作都携带数据源版本和策略上下文。
    pub async fn run_operation(
        &self,
        pack_id: &str,
        operation: DomainPackOperation,
    ) -> Result<AutomationJob, AutomationError> {
        let runtime = self.runtime_for_pack(pack_id).await?;
        if !runtime.enabled {
            return Err(AutomationError::Runtime(format!("业务包 {pack_id} 未启用")));
        }
        let file = self.authorized_source(&runtime)?;
        let fingerprint = file_fingerprint(&file)?;
        let mut payload = self.source_payload(&runtime, &file, &fingerprint).await?;
        let dedupe = if matches!(
            operation,
            DomainPackOperation::Analyze
                | DomainPackOperation::Backtest
                | DomainPackOperation::Review
        ) {
            let strategy_id = strategy_ids(&runtime)
                .into_iter()
                .next()
                .ok_or_else(|| AutomationError::Runtime("strategy_id_missing".into()))?;
            payload = stage_payload(&runtime, payload, &strategy_id)?;
            format!("manual:{fingerprint}:{strategy_id}")
        } else {
            if operation == DomainPackOperation::Sync {
                payload["auto_chain"] = Value::Bool(true);
            }
            format!("manual:{fingerprint}")
        };
        let (job, _) = self
            .storage
            .enqueue_automation_job(&runtime.pack_id, operation.as_str(), &dedupe, &payload, 3)
            .await?;
        Ok(job)
    }

    async fn scan_pack_internal(&self, runtime: &DomainPackRuntime) -> Result<(), AutomationError> {
        let file = self.authorized_source(runtime)?;
        let fingerprint = file_fingerprint(&file)?;
        let key = fingerprint_key(&runtime.pack_id);
        if self.storage.get_meta(&key).await?.as_deref() == Some(fingerprint.as_str()) {
            self.storage
                .set_meta(&last_success_key(&runtime.pack_id), &crate::trace::now_ts())
                .await?;
            return Ok(());
        }
        let mut payload = self.source_payload(runtime, &file, &fingerprint).await?;
        payload["auto_chain"] = Value::Bool(true);
        let (job, inserted) = self
            .storage
            .enqueue_automation_job(&runtime.pack_id, SYNC, &fingerprint, &payload, 3)
            .await?;
        self.storage.set_meta(&key, &fingerprint).await?;
        self.storage
            .set_meta(&last_success_key(&runtime.pack_id), &crate::trace::now_ts())
            .await?;
        if inserted {
            self.emit(
                "automation.source_changed",
                &job,
                json!({"fingerprint": fingerprint}),
            )
            .await;
        }
        Ok(())
    }

    async fn latest_scan_error(
        &self,
        pack_id: &str,
        fallback_source_path: &str,
    ) -> Result<Option<AutomationScanError>, AutomationError> {
        let events = self
            .storage
            .list_events(TraceQuery {
                project_id: Some(pack_id.to_string()),
                object_ref: Some(format!("domain-pack-runtime:{pack_id}")),
                event_type: Some("automation.source_scan_failed".into()),
                limit: 1,
                newest_first: true,
                ..Default::default()
            })
            .await?;
        let Some(event) = events.into_iter().next() else {
            return Ok(None);
        };
        let output = event.output.as_ref();
        let input = event.input.as_ref();
        Ok(Some(AutomationScanError {
            code: output
                .and_then(|value| value.get("error_code"))
                .and_then(Value::as_str)
                .unwrap_or("source_unavailable")
                .to_string(),
            message: output
                .and_then(|value| value.get("error"))
                .and_then(Value::as_str)
                .unwrap_or("数据源扫描失败")
                .to_string(),
            source_path: input
                .and_then(|value| value.get("source_path"))
                .and_then(Value::as_str)
                .unwrap_or(fallback_source_path)
                .to_string(),
            timestamp: event.timestamp,
            trace_id: event.id,
        }))
    }

    async fn scan_history(
        &self,
        pack_id: &str,
        fallback_source_path: &str,
    ) -> Result<Vec<AutomationScanHistory>, AutomationError> {
        let mut events = Vec::new();
        for event_type in ["automation.source_scan_failed", "automation.source_changed"] {
            events.extend(
                self.storage
                    .list_events(TraceQuery {
                        project_id: Some(pack_id.to_string()),
                        event_type: Some(event_type.into()),
                        limit: 20,
                        newest_first: true,
                        ..Default::default()
                    })
                    .await?,
            );
        }
        events.sort_by(|left, right| {
            timestamp_order(&right.timestamp, &left.timestamp).then_with(|| right.id.cmp(&left.id))
        });
        events.truncate(20);
        Ok(events
            .into_iter()
            .map(|event| {
                let input = event.input.as_ref();
                let output = event.output.as_ref();
                let failed = event.event_type == "automation.source_scan_failed";
                AutomationScanHistory {
                    status: if failed { "failed" } else { "succeeded" }.into(),
                    event_type: event.event_type,
                    code: output
                        .and_then(|value| value.get("error_code"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    message: output
                        .and_then(|value| value.get("error"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    fingerprint: output
                        .and_then(|value| value.get("fingerprint"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    source_path: input
                        .and_then(|value| value.get("source_path"))
                        .and_then(Value::as_str)
                        .or_else(|| {
                            (!fallback_source_path.is_empty()).then_some(fallback_source_path)
                        })
                        .map(str::to_owned),
                    timestamp: event.timestamp,
                    trace_id: event.id,
                    job_id: event.object_refs.first().cloned(),
                }
            })
            .collect())
    }

    async fn latest_scan_summary(
        &self,
        pack_id: &str,
        history: &[AutomationScanHistory],
    ) -> Result<Option<AutomationScanSummary>, AutomationError> {
        let latest_changed = history
            .iter()
            .find(|entry| entry.event_type == "automation.source_changed");
        let sync_job = self
            .storage
            .list_automation_jobs(Some(pack_id), 100)
            .await?
            .into_iter()
            .find(|job| job.job_type == SYNC);
        let Some(sync_job) = sync_job else {
            return Ok(latest_changed.map(|entry| AutomationScanSummary {
                source_rows: None,
                new_issue_count: None,
                duplicate_issues: None,
                fingerprint: entry.fingerprint.clone(),
                timestamp: entry.timestamp.clone(),
                trace_id: Some(entry.trace_id.clone()),
            }));
        };
        let entry = latest_changed;
        let payload = &sync_job.payload;
        let fingerprint = entry
            .and_then(|value| value.fingerprint.clone())
            .or_else(|| value_string(payload, "fingerprint"));
        let timestamp = entry
            .map(|value| value.timestamp.clone())
            .unwrap_or_else(|| sync_job.created_at.clone());
        let trace_id = entry.map(|value| value.trace_id.clone());
        Ok(Some(AutomationScanSummary {
            source_rows: value_u64(payload, "source_rows"),
            new_issue_count: value_u64(payload, "new_issue_count"),
            duplicate_issues: value_u64(payload, "duplicate_issues"),
            fingerprint,
            timestamp,
            trace_id,
        }))
    }

    async fn manual_retry_count(&self, pack_id: &str) -> Result<u64, AutomationError> {
        Ok(self
            .storage
            .list_events(TraceQuery {
                project_id: Some(pack_id.to_string()),
                event_type: Some("automation.source_scan_manual_retry".into()),
                limit: 1000,
                newest_first: false,
                ..Default::default()
            })
            .await?
            .len() as u64)
    }

    async fn manual_retry_info(
        &self,
        pack_id: &str,
    ) -> Result<(u64, Option<AutomationManualRetry>), AutomationError> {
        let events = self
            .storage
            .list_events(TraceQuery {
                project_id: Some(pack_id.to_string()),
                event_type: Some("automation.source_scan_manual_retry".into()),
                limit: 1000,
                newest_first: true,
                ..Default::default()
            })
            .await?;
        let count = events.len() as u64;
        let latest = events.into_iter().next().map(|event| {
            let input = event.input.as_ref();
            let output = event.output.as_ref();
            AutomationManualRetry {
                actor: input
                    .and_then(|value| value.get("actor"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned(),
                retry_number: input
                    .and_then(|value| value.get("retry_number"))
                    .and_then(Value::as_u64)
                    .unwrap_or(count),
                status: output
                    .and_then(|value| value.get("result"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned(),
                submitted_jobs: output
                    .and_then(|value| value.get("submitted_jobs"))
                    .and_then(Value::as_u64)
                    .unwrap_or_default(),
                timestamp: event.timestamp,
                trace_id: event.id,
            }
        });
        Ok((count, latest))
    }

    async fn emit_manual_retry(
        &self,
        runtime: &DomainPackRuntime,
        actor: &str,
        retry_number: u64,
        submitted_jobs: u64,
        result: &str,
        error: Option<&str>,
    ) {
        let event = crate::trace::TraceEvent {
            id: crate::trace::new_trace_id(),
            kind: crate::trace::TraceKind::World,
            event_type: "automation.source_scan_manual_retry".into(),
            object_refs: vec![format!("domain-pack-runtime:{}", runtime.pack_id)],
            project_id: Some(runtime.pack_id.clone()),
            run_id: None,
            agent_id: Some("automation-engine".into()),
            reasoning: None,
            input: Some(json!({
                "pack_id": runtime.pack_id,
                "source_path": runtime.source_path,
                "actor": actor,
                "retry_number": retry_number,
            })),
            output: Some(json!({
                "submitted_jobs": submitted_jobs,
                "result": result,
                "error": error,
            })),
            confidence: None,
            timestamp: crate::trace::now_ts(),
        };
        if self.storage.append_trace(&event).await.is_ok() {
            let _ = self.event_tx.send(event);
        }
    }

    fn authorized_source(&self, runtime: &DomainPackRuntime) -> Result<PathBuf, AutomationError> {
        let file = canonical_source(&runtime.source_path)?;
        let parent = file
            .parent()
            .ok_or_else(|| AutomationError::Runtime("source_invalid: 数据源缺少父目录".into()))?;
        self.plugin_catalog
            .allow_filesystem_read_root(&runtime.pack_id, parent)
            .and_then(|_| {
                self.plugin_catalog
                    .authorize_filesystem_read(&runtime.pack_id, &file)
            })
            .map_err(|error| AutomationError::Runtime(format!("数据源授权失败：{error}")))
    }

    async fn run_one(&self) -> Result<(), AutomationError> {
        let Some(job) = self.storage.claim_next_automation_job().await? else {
            return Ok(());
        };
        self.emit("automation.job_started", &job, Value::Null).await;
        match self.execute_job(&job).await {
            Ok(result) => {
                self.storage
                    .complete_automation_job(&job.id, &result)
                    .await?;
                self.record_automation_run(&job, "succeeded", Some(&result), None)
                    .await?;
                self.emit("automation.job_succeeded", &job, result).await;
            }
            Err(error) => {
                let message = error.to_string();
                self.storage
                    .fail_automation_job(&job.id, &message, retry_delay(job.attempt))
                    .await?;
                self.record_automation_run(&job, "failed", None, Some(&message))
                    .await?;
                self.emit("automation.job_failed", &job, json!({"error": message}))
                    .await;
            }
        }
        Ok(())
    }

    async fn execute_job(&self, job: &AutomationJob) -> Result<Value, AutomationError> {
        let runtime = self.runtime_for_pack(&job.pack_id).await?;
        match job.job_type.as_str() {
            SYNC => {
                let result = self.invoke_plugin(job, "lottery.data.sync").await?;
                let queued = self.enqueue_next_stage(&runtime, job).await?;
                Ok(json!({
                    "result": result,
                    "queued": queued.into_iter().map(|job| job.id).collect::<Vec<_>>(),
                    "strategy_isolation": true,
                }))
            }
            IMPORT => self.invoke_plugin(job, "lottery.import").await,
            ANALYZE => {
                ensure_supported_strategy(job)?;
                let result = self.invoke_plugin(job, "lottery.analyze").await?;
                let queued = if auto_chain(job) {
                    self.enqueue_next_stage(&runtime, job).await?
                } else {
                    Vec::new()
                };
                Ok(json!({
                    "result": result,
                    "queued": queued.into_iter().map(|job| job.id).collect::<Vec<_>>(),
                    "strategy_isolation": true,
                }))
            }
            BACKTEST => {
                ensure_supported_strategy(job)?;
                let result = self.invoke_plugin(job, "lottery.backtest").await?;
                let queued = if auto_chain(job) {
                    self.enqueue_next_stage(&runtime, job).await?
                } else {
                    Vec::new()
                };
                Ok(json!({
                    "result": result,
                    "queued": queued.into_iter().map(|job| job.id).collect::<Vec<_>>(),
                    "strategy_isolation": true,
                }))
            }
            REVIEW => {
                ensure_supported_strategy(job)?;
                self.invoke_plugin(job, "lottery.review").await
            }
            other => Err(AutomationError::Runtime(format!(
                "未知自动化任务类型：{other}"
            ))),
        }
    }

    async fn enqueue_stage(
        &self,
        runtime: &DomainPackRuntime,
        job_type: &str,
        source_job: &AutomationJob,
        strategy_id: &str,
    ) -> Result<AutomationJob, AutomationError> {
        let fingerprint = source_job
            .payload
            .get("fingerprint")
            .and_then(Value::as_str)
            .unwrap_or(&source_job.dedupe_key);
        let payload = stage_payload(runtime, source_job.payload.clone(), strategy_id)?;
        let dedupe = format!("{fingerprint}:{strategy_id}");
        let (job, inserted) = self
            .storage
            .enqueue_automation_job(&runtime.pack_id, job_type, &dedupe, &payload, 3)
            .await?;
        if inserted {
            self.emit("automation.job_queued", &job, Value::Null).await;
        }
        Ok(job)
    }

    async fn enqueue_next_stage(
        &self,
        runtime: &DomainPackRuntime,
        source_job: &AutomationJob,
    ) -> Result<Vec<AutomationJob>, AutomationError> {
        let Some(job_type) = next_stage(runtime, source_job.job_type.as_str()) else {
            return Ok(Vec::new());
        };
        let mut queued = Vec::new();
        for strategy_id in strategy_ids(runtime) {
            queued.push(
                self.enqueue_stage(runtime, job_type.as_str(), source_job, &strategy_id)
                    .await?,
            );
        }
        Ok(queued)
    }

    async fn invoke_plugin(
        &self,
        job: &AutomationJob,
        capability: &str,
    ) -> Result<Value, AutomationError> {
        self.plugin_catalog
            .invoke(&job.pack_id, capability, job.payload.clone())
            .await
            .map_err(|error| AutomationError::Runtime(error.to_string()))
    }

    async fn source_payload(
        &self,
        runtime: &DomainPackRuntime,
        file: &Path,
        fingerprint: &str,
    ) -> Result<Value, AutomationError> {
        let game = game_for_source(file);
        let inspection = crate::lottery::inspect_csv(&self.storage, game, file, &runtime.pack_id)
            .await
            .map_err(|error| {
                AutomationError::Runtime(format!("source_invalid: 数据源检查失败：{error}"))
            })?;
        if let Some(issue) = inspection.duplicate_issues.first() {
            return Err(AutomationError::Runtime(format!(
                "source_invalid: CSV 中存在重复期号：{issue}"
            )));
        }
        Ok(json!({
            "pack_id": runtime.pack_id,
            "project": runtime.pack_id,
            "file": file,
            "game": game.code(),
            "fingerprint": fingerprint,
            "input_version": fingerprint,
            "window": DEFAULT_WINDOW,
            "train_window": DEFAULT_WINDOW,
            "test_count": DEFAULT_TEST_COUNT,
            "source_rows": inspection.row_count,
            "new_issue_count": inspection.new_issue_count,
            "duplicate_issues": inspection.duplicate_issues,
            "source_issues": inspection.issues,
        }))
    }

    async fn record_automation_run(
        &self,
        job: &AutomationJob,
        status: &str,
        result: Option<&Value>,
        error: Option<&str>,
    ) -> Result<(), AutomationError> {
        let id = format!("domain-pack-run:{}", job.id);
        let output = result.and_then(|value| value.get("result")).or(result);
        let data = json!({
            "job_id": job.id,
            "pack_id": job.pack_id,
            "operation": job.job_type,
            "status": status,
            "input_version": job.payload.get("input_version").or_else(|| job.payload.get("fingerprint")),
            "strategy_id": job.payload.get("strategy_id"),
            "strategy_family": job.payload.get("strategy_family"),
            "strategy_version": job.payload.get("strategy_version"),
            "parameters": {
                "window": job.payload.get("window"),
                "train_window": job.payload.get("train_window"),
                "test_count": job.payload.get("test_count"),
            },
            "output_summary": result,
            "hit_results": output.and_then(|value| value.get("mean_red_hits")),
            "baseline_comparison": output.and_then(|value| value.get("random_baseline_red_hits")),
            "error": error,
            "pi_review": if job.job_type == REVIEW { result } else { None },
            "conclusion": if status == "succeeded" { "completed" } else { "failed" },
            "created_at": crate::trace::now_ts(),
        });
        let command = if self.storage.get(&id).await?.is_some() {
            WorldCommand::UpdateObject {
                id,
                data,
                project_id: Some(job.pack_id.clone()),
            }
        } else {
            WorldCommand::CreateObject {
                object_type: "AutomationRun".into(),
                id,
                data,
                project_id: Some(job.pack_id.clone()),
            }
        };
        self.storage.apply_world(command).await?;
        Ok(())
    }

    async fn runtime_for_pack(&self, pack_id: &str) -> Result<DomainPackRuntime, AutomationError> {
        let id = format!("domain-pack-runtime:{pack_id}");
        let Some(record) = self.storage.get(&id).await? else {
            return Ok(DomainPackRuntime {
                pack_id: pack_id.to_string(),
                enabled: true,
                source_path: String::new(),
                watch_enabled: false,
                poll_interval_seconds: 30,
                auto_sync: false,
                auto_analyze: false,
                auto_backtest: false,
                auto_review: false,
                strategy_ids: Vec::new(),
                updated_by: String::new(),
                updated_at: String::new(),
            });
        };
        serde_json::from_value(record.data).map_err(AutomationError::Json)
    }

    async fn emit(&self, event_type: &str, job: &AutomationJob, output: Value) {
        let event = crate::trace::TraceEvent {
            id: crate::trace::new_trace_id(),
            kind: crate::trace::TraceKind::World,
            event_type: event_type.to_string(),
            object_refs: vec![job.id.clone()],
            project_id: Some(job.pack_id.clone()),
            run_id: None,
            agent_id: Some("automation-engine".into()),
            reasoning: None,
            input: Some(
                json!({"jobType": job.job_type, "dedupeKey": job.dedupe_key, "attempt": job.attempt}),
            ),
            output: Some(output),
            confidence: None,
            timestamp: crate::trace::now_ts(),
        };
        if self.storage.append_trace(&event).await.is_ok() {
            let _ = self.event_tx.send(event);
        }
    }

    async fn emit_scan_error(&self, runtime: &DomainPackRuntime, error: &AutomationError) {
        let event = crate::trace::TraceEvent {
            id: crate::trace::new_trace_id(),
            kind: crate::trace::TraceKind::World,
            event_type: "automation.source_scan_failed".into(),
            object_refs: vec![format!("domain-pack-runtime:{}", runtime.pack_id)],
            project_id: Some(runtime.pack_id.clone()),
            run_id: None,
            agent_id: Some("automation-engine".into()),
            reasoning: None,
            input: Some(json!({"source_path": runtime.source_path})),
            output: Some(json!({
                "error_code": scan_error_code(error),
                "error": error.to_string(),
            })),
            confidence: None,
            timestamp: crate::trace::now_ts(),
        };
        if self.storage.append_trace(&event).await.is_ok() {
            let _ = self.event_tx.send(event);
        }
    }
}

fn timestamp_order(left: &str, right: &str) -> Ordering {
    match (left.parse::<u128>(), right.parse::<u128>()) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

fn automation_quality(jobs: &[AutomationJob]) -> AutomationQuality {
    let mut quality = AutomationQuality {
        total_jobs: jobs.len() as u64,
        ..AutomationQuality::default()
    };
    for job in jobs {
        match job.status.as_str() {
            "succeeded" => quality.succeeded_jobs += 1,
            "failed" => quality.failed_jobs += 1,
            "queued" => quality.queued_jobs += 1,
            "running" => quality.running_jobs += 1,
            "cancelled" => quality.cancelled_jobs += 1,
            _ => {}
        }
        if job.attempt > 1 {
            quality.retry_jobs += 1;
        }
        if job.job_type == REVIEW && quality.last_review_status.is_none() {
            quality.last_review_status = Some(job.status.clone());
            quality.last_review_at = Some(
                job.finished_at
                    .clone()
                    .or_else(|| job.started_at.clone())
                    .unwrap_or_else(|| job.updated_at.clone()),
            );
        }
    }
    let terminal_jobs = quality.succeeded_jobs + quality.failed_jobs + quality.cancelled_jobs;
    quality.success_rate = if terminal_jobs == 0 {
        0.0
    } else {
        quality.succeeded_jobs as f64 / terminal_jobs as f64
    };
    quality
}

fn scan_state(
    last_success_at: Option<&str>,
    last_scan_error: Option<&AutomationScanError>,
) -> String {
    match (last_success_at, last_scan_error) {
        (None, None) => "never".into(),
        (Some(_), None) => "healthy".into(),
        (None, Some(_)) => "failed".into(),
        (Some(success), Some(error)) => {
            if timestamp_order(success, &error.timestamp) == Ordering::Greater {
                "recovered".into()
            } else {
                "failed".into()
            }
        }
    }
}

fn value_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn value_u64(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64).or_else(|| {
        value
            .get(key)
            .and_then(Value::as_i64)
            .and_then(|value| u64::try_from(value).ok())
    })
}

fn pack_directories(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut dirs = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs
}

fn canonical_source(source: &str) -> Result<PathBuf, AutomationError> {
    if source.trim().is_empty() {
        return Err(AutomationError::Runtime("业务包未配置 source_path".into()));
    }
    let path = std::fs::canonicalize(source).map_err(|error| {
        let code = match error.kind() {
            std::io::ErrorKind::NotFound => "file_not_found",
            std::io::ErrorKind::PermissionDenied => "file_permission_denied",
            _ => "source_unavailable",
        };
        AutomationError::Runtime(format!("{code}: {error}"))
    })?;
    if !path.is_file() {
        return Err(AutomationError::Runtime(format!(
            "数据源不是文件：{}",
            path.display()
        )));
    }
    Ok(path)
}

fn file_fingerprint(path: &Path) -> Result<String, AutomationError> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| AutomationError::Runtime(format!("读取数据源元数据失败：{error}")))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    Ok(format!("{}:{}", modified, metadata.len()))
}

fn game_for_source(file: &Path) -> crate::lottery::LotteryGame {
    let file_name = file.to_string_lossy().to_ascii_lowercase();
    if file_name.contains("double") || file_name.contains("ssq") {
        crate::lottery::LotteryGame::Double
    } else {
        crate::lottery::LotteryGame::Dlt
    }
}

fn strategy_ids(runtime: &DomainPackRuntime) -> Vec<String> {
    let mut ids = Vec::new();
    for id in &runtime.strategy_ids {
        let id = id.trim();
        if !id.is_empty() && !ids.iter().any(|existing| existing == id) {
            ids.push(id.to_string());
        }
    }
    if ids.is_empty() {
        ids.push("statistics".into());
    }
    ids
}

fn strategy_family(strategy_id: &str) -> String {
    match strategy_id {
        "statistics" => "statistics".into(),
        "heuristic" => "heuristic".into(),
        "metaphysics" => "metaphysics".into(),
        _ => "custom".into(),
    }
}

fn stage_payload(
    runtime: &DomainPackRuntime,
    payload: Value,
    strategy_id: &str,
) -> Result<Value, AutomationError> {
    if strategy_id.trim().is_empty() {
        return Err(AutomationError::Runtime("strategy_id 不能为空".into()));
    }
    let mut payload = payload;
    if let Some(object) = payload.as_object_mut() {
        object.insert("project".into(), Value::String(runtime.pack_id.clone()));
        object.insert("strategy_id".into(), Value::String(strategy_id.into()));
        object.insert(
            "strategy_family".into(),
            Value::String(strategy_family(strategy_id)),
        );
        object.insert("strategy_version".into(), Value::String("1.0.0".into()));
        object.insert("training_window".into(), json!(DEFAULT_WINDOW));
        object.insert("train_window".into(), json!(DEFAULT_WINDOW));
        object.insert("strategy_isolation".into(), Value::Bool(true));
    }
    Ok(payload)
}

fn next_stage(runtime: &DomainPackRuntime, current: &str) -> Option<String> {
    match current {
        SYNC => {
            if runtime.auto_analyze {
                Some(ANALYZE.to_string())
            } else if runtime.auto_backtest {
                Some(BACKTEST.to_string())
            } else if runtime.auto_review {
                Some(REVIEW.to_string())
            } else {
                None
            }
        }
        ANALYZE => {
            if runtime.auto_backtest {
                Some(BACKTEST.to_string())
            } else if runtime.auto_review {
                Some(REVIEW.to_string())
            } else {
                None
            }
        }
        BACKTEST if runtime.auto_review => Some(REVIEW.to_string()),
        _ => None,
    }
}

fn auto_chain(job: &AutomationJob) -> bool {
    job.payload
        .get("auto_chain")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn ensure_supported_strategy(job: &AutomationJob) -> Result<(), AutomationError> {
    let strategy_id = job
        .payload
        .get("strategy_id")
        .and_then(Value::as_str)
        .ok_or_else(|| AutomationError::Runtime("strategy_id_missing".into()))?;
    let family = job
        .payload
        .get("strategy_family")
        .and_then(Value::as_str)
        .ok_or_else(|| AutomationError::Runtime("strategy_family_missing".into()))?;
    if strategy_id != "statistics" || family != "statistics" {
        return Err(AutomationError::Runtime(format!(
            "strategy_not_implemented: {strategy_id}"
        )));
    }
    Ok(())
}

fn retry_delay(attempt: i64) -> u64 {
    2_u64.saturating_pow(attempt.clamp(0, 6) as u32)
}

fn scan_error_code(error: &AutomationError) -> &'static str {
    let message = error.to_string();
    if message.contains("file_not_found") {
        "file_not_found"
    } else if message.contains("file_permission_denied") {
        "file_permission_denied"
    } else if message.contains("权限拒绝") || message.contains("数据源授权失败") {
        "permission_denied"
    } else if message.contains("source_invalid") {
        "source_invalid"
    } else {
        "source_unavailable"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static PI_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn source_fingerprint_changes_with_size() {
        let dir = std::env::temp_dir().join(format!("automation_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("dlt.csv");
        std::fs::write(&file, "a").unwrap();
        let first = file_fingerprint(&file).unwrap();
        std::fs::write(&file, "ab").unwrap();
        let second = file_fingerprint(&file).unwrap();
        assert_ne!(first, second);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn strategy_payload_is_explicit_and_isolated() {
        let runtime = DomainPackRuntime {
            pack_id: "lottery-analysis".into(),
            enabled: true,
            source_path: "/tmp/dlt.csv".into(),
            watch_enabled: false,
            poll_interval_seconds: 30,
            auto_sync: false,
            auto_analyze: true,
            auto_backtest: true,
            auto_review: false,
            strategy_ids: vec!["statistics".into()],
            updated_by: "test".into(),
            updated_at: "".into(),
        };
        let payload = stage_payload(&runtime, json!({"game": "dlt"}), "statistics").unwrap();
        assert_eq!(payload["strategy_id"], "statistics");
        assert_eq!(payload["strategy_family"], "statistics");
        assert_eq!(payload["strategy_version"], "1.0.0");
        assert_eq!(payload["strategy_isolation"], true);
    }

    #[test]
    fn recovered_scan_has_no_active_error() {
        let error = AutomationScanError {
            code: "permission_denied".into(),
            message: "拒绝读取".into(),
            source_path: "/tmp/dlt.csv".into(),
            timestamp: "2".into(),
            trace_id: "trace-2".into(),
        };
        assert_eq!(scan_state(None, Some(&error)), "failed");
        assert_eq!(scan_state(Some("3"), Some(&error)), "recovered");
    }

    #[test]
    fn automation_quality_counts_status_and_retries() {
        let job = |job_type: &str, status: &str, attempt: i64, updated_at: &str| AutomationJob {
            id: format!("test:{job_type}:{updated_at}"),
            pack_id: "lottery-analysis".into(),
            job_type: job_type.into(),
            dedupe_key: updated_at.into(),
            payload: Value::Null,
            status: status.into(),
            attempt,
            max_attempts: 3,
            available_at: updated_at.into(),
            started_at: None,
            finished_at: (status == "succeeded").then(|| updated_at.into()),
            last_error: None,
            result: None,
            created_at: updated_at.into(),
            updated_at: updated_at.into(),
        };
        let quality = automation_quality(&[
            job(SYNC, "succeeded", 1, "1"),
            job(ANALYZE, "queued", 2, "2"),
            job(REVIEW, "failed", 3, "3"),
            job(BACKTEST, "cancelled", 1, "4"),
        ]);
        assert_eq!(quality.total_jobs, 4);
        assert_eq!(quality.succeeded_jobs, 1);
        assert_eq!(quality.failed_jobs, 1);
        assert_eq!(quality.queued_jobs, 1);
        assert_eq!(quality.running_jobs, 0);
        assert_eq!(quality.cancelled_jobs, 1);
        assert_eq!(quality.retry_jobs, 2);
        assert!((quality.success_rate - (1.0 / 3.0)).abs() < f64::EPSILON);
        assert_eq!(quality.last_review_status.as_deref(), Some("failed"));
        assert_eq!(quality.last_review_at.as_deref(), Some("3"));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // PI_ENV_LOCK must be held across the whole test
    async fn watcher_detects_real_file_changes_and_runs_default_chain() {
        use crate::plugin::catalog::PluginCatalog;
        use crate::storage::ObjectEngine;

        let _pi_guard = PI_ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var(
                "PI_BIN",
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/v2_parity/fixtures/fake_pi_lottery.sh"),
            );
        }
        let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
        let root = std::env::temp_dir().join(format!(
            "automation_watcher_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let packs_dir = root.join("packs");
        let pack_dir = packs_dir.join("lottery-analysis");
        let data_dir = root.join("data");
        std::fs::create_dir_all(&pack_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();

        let source_pack =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("domain-packs/lottery-analysis");
        for file_name in ["manifest.json", "ontology.odl", "bootstrap.json"] {
            std::fs::copy(source_pack.join(file_name), pack_dir.join(file_name)).unwrap();
        }

        let source_file = data_dir.join("dlt.csv");
        let mut csv = String::from("期号,前区1,前区2,前区3,前区4,前区5,后区1,后区2,日期\n");
        for issue in 1..=90 {
            csv.push_str(&format!("{issue},01,02,03,04,05,01,02,2026-01-01\n"));
        }
        std::fs::write(&source_file, csv).unwrap();

        let pack = DomainPack::load(&pack_dir).unwrap();
        pack.install(storage.as_ref()).await.unwrap();
        let runtime = DomainPackRuntime {
            pack_id: "lottery-analysis".into(),
            enabled: true,
            source_path: source_file.to_string_lossy().into_owned(),
            watch_enabled: true,
            poll_interval_seconds: 0,
            auto_sync: true,
            auto_analyze: true,
            auto_backtest: true,
            auto_review: true,
            strategy_ids: vec!["statistics".into()],
            updated_by: "test".into(),
            updated_at: crate::trace::now_ts(),
        };
        storage
            .create(
                "DomainPackRuntime",
                "domain-pack-runtime:lottery-analysis",
                serde_json::to_value(runtime).unwrap(),
            )
            .await
            .unwrap();

        let source_root = source_file.parent().unwrap().to_path_buf();
        let catalog = Arc::new(
            PluginCatalog::with_builtins(storage.as_ref().clone(), [source_root]).unwrap(),
        );
        let (event_tx, _) = tokio::sync::broadcast::channel(32);
        let engine = Arc::new(AutomationEngine::new(
            storage.clone(),
            catalog,
            packs_dir,
            event_tx,
        ));
        let mut last_scan = HashMap::new();

        engine.scan_due(&mut last_scan).await.unwrap();
        assert_eq!(
            storage
                .list_automation_jobs(Some("lottery-analysis"), 20)
                .await
                .unwrap()
                .len(),
            1,
            "首次文件扫描必须只入队一个 sync"
        );
        for _ in 0..4 {
            engine.run_one().await.unwrap();
        }

        let first_jobs = storage
            .list_automation_jobs(Some("lottery-analysis"), 20)
            .await
            .unwrap();
        assert_eq!(first_jobs.len(), 4, "sync 后应自动排队完整四阶段链路");
        assert!(first_jobs.iter().all(|job| job.status == "succeeded"));
        let review = first_jobs
            .iter()
            .find(|job| job.job_type == REVIEW)
            .expect("自动链路必须包含 review");
        assert_eq!(review.payload["strategy_id"], "statistics");
        assert_eq!(review.payload["strategy_family"], "statistics");
        assert_eq!(review.payload["strategy_isolation"], true);
        assert_eq!(
            review.result.as_ref().unwrap()["capability"],
            "lottery.review"
        );
        assert_eq!(review.result.as_ref().unwrap()["confidence"], 0.9);
        assert_eq!(storage.query("LotteryDraw").await.unwrap().len(), 90);
        let quality = engine
            .pack_status("lottery-analysis")
            .await
            .unwrap()
            .quality;
        assert_eq!(quality.total_jobs, 4);
        assert_eq!(quality.succeeded_jobs, 4);
        assert_eq!(quality.failed_jobs, 0);
        assert_eq!(quality.last_review_status.as_deref(), Some("succeeded"));

        engine.scan_due(&mut last_scan).await.unwrap();
        assert_eq!(
            storage
                .list_automation_jobs(Some("lottery-analysis"), 20)
                .await
                .unwrap()
                .len(),
            4,
            "相同 fingerprint 重复扫描不得重复入队"
        );

        let mut updated = std::fs::read_to_string(&source_file).unwrap();
        updated.push_str("91,01,02,03,04,05,01,02,2026-01-01\n");
        std::fs::write(&source_file, updated).unwrap();
        engine.scan_due(&mut last_scan).await.unwrap();
        let changed_jobs = storage
            .list_automation_jobs(Some("lottery-analysis"), 20)
            .await
            .unwrap();
        assert_eq!(
            changed_jobs.len(),
            5,
            "文件 fingerprint 变化后应新增一个 sync"
        );
        let changed_sync = changed_jobs
            .iter()
            .find(|job| job.job_type == SYNC && job.payload["new_issue_count"] == 1)
            .expect("新增期号应进入新的 sync payload");
        assert_eq!(changed_sync.status, "queued");

        for _ in 0..4 {
            engine.run_one().await.unwrap();
        }
        let final_jobs = storage
            .list_automation_jobs(Some("lottery-analysis"), 20)
            .await
            .unwrap();
        assert_eq!(final_jobs.len(), 8, "第二次 sync 也应完成完整四阶段链路");
        assert!(final_jobs.iter().all(|job| job.status == "succeeded"));
        assert_eq!(storage.query("LotteryDraw").await.unwrap().len(), 91);
        let quality = engine
            .pack_status("lottery-analysis")
            .await
            .unwrap()
            .quality;
        assert_eq!(quality.total_jobs, 8);
        assert_eq!(quality.succeeded_jobs, 8);
        assert_eq!(quality.last_review_status.as_deref(), Some("succeeded"));

        unsafe { std::env::remove_var("PI_BIN") };
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn configured_lottery_pack_scans_and_dispatches_sync() {
        use crate::domain_pack::DomainPackRuntime;
        use crate::plugin::catalog::PluginCatalog;
        use crate::storage::ObjectEngine;

        let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
        storage
            .create_project(
                "lottery-analysis",
                "彩票",
                "测试",
                "object Lottery:\n  properties:\n    source: string",
                None,
            )
            .await
            .unwrap();
        let dir = std::env::temp_dir().join(format!("automation_e2e_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("dlt.csv");
        std::fs::write(
            &file,
            "期号,前区1,前区2,前区3,前区4,前区5,后区1,后区2,日期\n1,01,02,03,04,05,01,02,2026-01-01\n2,01,02,06,07,08,01,03,2026-01-03\n3,02,04,06,08,10,02,04,2026-01-05\n",
        )
        .unwrap();
        let runtime = DomainPackRuntime {
            pack_id: "lottery-analysis".into(),
            enabled: true,
            source_path: file.to_string_lossy().into_owned(),
            watch_enabled: true,
            poll_interval_seconds: 1,
            auto_sync: true,
            auto_analyze: false,
            auto_backtest: false,
            auto_review: false,
            strategy_ids: vec![],
            updated_by: "test".into(),
            updated_at: crate::trace::now_ts(),
        };
        storage
            .create(
                "DomainPackRuntime",
                "domain-pack-runtime:lottery-analysis",
                serde_json::to_value(runtime).unwrap(),
            )
            .await
            .unwrap();
        let source_root = file.parent().unwrap().to_path_buf();
        let catalog = Arc::new(
            PluginCatalog::with_builtins(storage.as_ref().clone(), [source_root]).unwrap(),
        );
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let engine = Arc::new(AutomationEngine::new(
            storage.clone(),
            catalog,
            dir.clone(),
            event_tx,
        ));
        let jobs = engine
            .scan_pack_by("lottery-analysis", Some("admin"))
            .await
            .unwrap();
        assert_eq!(jobs.len(), 1);
        let status = engine.pack_status("lottery-analysis").await.unwrap();
        assert!(status.fingerprint.is_some());
        assert!(status.last_success_at.is_some());
        assert!(status.last_scan_error.is_none());
        assert_eq!(status.scan_state, "healthy");
        assert_eq!(status.manual_retry_count, 1);
        assert_eq!(status.last_manual_retry.as_ref().unwrap().actor, "admin");
        assert_eq!(status.scan_history.len(), 1);
        assert_eq!(status.scan_history[0].status, "succeeded");
        assert!(status.last_scan_summary.is_some());
        engine.run_one().await.unwrap();
        engine.run_one().await.unwrap();
        assert_eq!(storage.query("LotteryDraw").await.unwrap().len(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn watcher_rejects_unauthorized_source_before_csv_read() {
        use crate::lottery_plugin::{LotteryPlugin, declare_capabilities};
        use crate::plugin::catalog::PluginCatalog;
        use crate::storage::ObjectEngine;

        let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
        let root = std::env::temp_dir().join(format!(
            "automation_permission_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let packs_dir = root.join("packs");
        let pack_dir = packs_dir.join("lottery-analysis");
        let allowed_dir = root.join("allowed");
        let forbidden_dir = root.join("forbidden");
        std::fs::create_dir_all(&pack_dir).unwrap();
        std::fs::create_dir_all(&allowed_dir).unwrap();
        std::fs::create_dir_all(&forbidden_dir).unwrap();
        let source_pack =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("domain-packs/lottery-analysis");
        for file_name in ["manifest.json", "ontology.odl", "bootstrap.json"] {
            std::fs::copy(source_pack.join(file_name), pack_dir.join(file_name)).unwrap();
        }
        let source_file = forbidden_dir.join("dlt.csv");
        std::fs::write(
            &source_file,
            "期号,前区1,前区2,前区3,前区4,前区5,后区1,后区2,日期\n1,01,02,03,04,05,01,02,2026-01-01\n",
        )
        .unwrap();

        DomainPack::load(&pack_dir)
            .unwrap()
            .install(storage.as_ref())
            .await
            .unwrap();
        storage
            .create(
                "DomainPackRuntime",
                "domain-pack-runtime:lottery-analysis",
                serde_json::to_value(DomainPackRuntime {
                    pack_id: "lottery-analysis".into(),
                    enabled: true,
                    source_path: source_file.to_string_lossy().into_owned(),
                    watch_enabled: true,
                    poll_interval_seconds: 0,
                    auto_sync: true,
                    auto_analyze: false,
                    auto_backtest: false,
                    auto_review: false,
                    strategy_ids: vec![],
                    updated_by: "test".into(),
                    updated_at: crate::trace::now_ts(),
                })
                .unwrap(),
            )
            .await
            .unwrap();

        let mut catalog = PluginCatalog::new();
        catalog
            .register_async(
                Box::new(LotteryPlugin::with_source_roots(
                    storage.as_ref().clone(),
                    [allowed_dir],
                )),
                declare_capabilities,
            )
            .unwrap();
        let catalog = Arc::new(catalog);
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let engine = AutomationEngine::new(storage.clone(), catalog, packs_dir, event_tx);
        let mut last_scan = HashMap::new();

        engine.scan_due(&mut last_scan).await.unwrap();
        assert!(
            storage
                .list_automation_jobs(Some("lottery-analysis"), 20)
                .await
                .unwrap()
                .is_empty(),
            "未授权文件不能在权限检查前进入 CSV 检查或任务队列"
        );
        assert_eq!(
            storage
                .get_meta("automation:fingerprint:lottery-analysis")
                .await
                .unwrap(),
            None,
            "权限失败不得污染 fingerprint"
        );
        let scan_error = storage
            .list_all_events()
            .await
            .unwrap()
            .into_iter()
            .find(|event| event.event_type == "automation.source_scan_failed")
            .expect("watcher 错误必须写入审计事件");
        assert_eq!(
            scan_error.output.unwrap()["error_code"],
            "permission_denied"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn watcher_retries_missing_and_invalid_source_after_repair() {
        use crate::lottery_plugin::{LotteryPlugin, declare_capabilities};
        use crate::plugin::catalog::PluginCatalog;
        use crate::storage::ObjectEngine;

        let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
        let root = std::env::temp_dir().join(format!(
            "automation_repair_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let packs_dir = root.join("packs");
        let pack_dir = packs_dir.join("lottery-analysis");
        let data_dir = root.join("data");
        std::fs::create_dir_all(&pack_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        let source_pack =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("domain-packs/lottery-analysis");
        for file_name in ["manifest.json", "ontology.odl", "bootstrap.json"] {
            std::fs::copy(source_pack.join(file_name), pack_dir.join(file_name)).unwrap();
        }
        let source_file = data_dir.join("dlt.csv");

        DomainPack::load(&pack_dir)
            .unwrap()
            .install(storage.as_ref())
            .await
            .unwrap();
        storage
            .create(
                "DomainPackRuntime",
                "domain-pack-runtime:lottery-analysis",
                serde_json::to_value(DomainPackRuntime {
                    pack_id: "lottery-analysis".into(),
                    enabled: true,
                    source_path: source_file.to_string_lossy().into_owned(),
                    watch_enabled: true,
                    poll_interval_seconds: 0,
                    auto_sync: true,
                    auto_analyze: false,
                    auto_backtest: false,
                    auto_review: false,
                    strategy_ids: vec![],
                    updated_by: "test".into(),
                    updated_at: crate::trace::now_ts(),
                })
                .unwrap(),
            )
            .await
            .unwrap();

        let mut catalog = PluginCatalog::new();
        catalog
            .register_async(
                Box::new(LotteryPlugin::with_source_roots(
                    storage.as_ref().clone(),
                    [data_dir.clone()],
                )),
                declare_capabilities,
            )
            .unwrap();
        let catalog = Arc::new(catalog);
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let engine = AutomationEngine::new(storage.clone(), catalog, packs_dir, event_tx);
        let mut last_scan = HashMap::new();

        engine.scan_due(&mut last_scan).await.unwrap();
        assert!(
            storage
                .list_automation_jobs(Some("lottery-analysis"), 20)
                .await
                .unwrap()
                .is_empty(),
            "文件不存在时 watcher 应记录失败并等待下一次扫描"
        );
        assert!(last_scan.is_empty());
        let missing_status = engine.pack_status("lottery-analysis").await.unwrap();
        let missing_error = missing_status
            .last_scan_error
            .expect("缺失文件应暴露扫描错误");
        assert_eq!(missing_error.code, "file_not_found");
        assert_eq!(missing_error.source_path, source_file.to_string_lossy());
        assert!(missing_status.fingerprint.is_none());
        assert!(missing_status.last_success_at.is_none());

        std::fs::write(&source_file, "not,a,lottery,csv\n").unwrap();
        engine.scan_due(&mut last_scan).await.unwrap();
        assert!(
            storage
                .list_automation_jobs(Some("lottery-analysis"), 20)
                .await
                .unwrap()
                .is_empty(),
            "CSV 格式错误时不得入队"
        );
        assert_eq!(
            storage
                .get_meta("automation:fingerprint:lottery-analysis")
                .await
                .unwrap(),
            None,
            "CSV 格式错误不得污染 fingerprint"
        );

        std::fs::write(
            &source_file,
            "期号,前区1,前区2,前区3,前区4,前区5,后区1,后区2,日期\n1,01,02,03,04,05,01,02,2026-01-01\n1,02,03,04,05,06,02,03,2026-01-03\n",
        )
        .unwrap();
        engine.scan_due(&mut last_scan).await.unwrap();
        assert!(
            storage
                .list_automation_jobs(Some("lottery-analysis"), 20)
                .await
                .unwrap()
                .is_empty(),
            "重复期号时不得入队"
        );
        let duplicate_status = engine.pack_status("lottery-analysis").await.unwrap();
        assert_eq!(
            duplicate_status
                .last_scan_error
                .as_ref()
                .map(|error| error.code.as_str()),
            Some("source_invalid")
        );
        assert!(
            duplicate_status
                .last_scan_error
                .as_ref()
                .is_some_and(|error| error.message.contains("重复期号")),
            "重复期号错误必须对操作界面可见"
        );

        std::fs::write(
            &source_file,
            "期号,前区1,前区2,前区3,前区4,前区5,后区1,后区2,日期\n1,01,02,03,04,05,01,02,2026-01-01\n",
        )
        .unwrap();
        engine.scan_due(&mut last_scan).await.unwrap();
        assert_eq!(
            storage
                .list_automation_jobs(Some("lottery-analysis"), 20)
                .await
                .unwrap()
                .len(),
            1,
            "修复 CSV 后下一次扫描应成功入队"
        );
        assert!(
            storage
                .get_meta("automation:fingerprint:lottery-analysis")
                .await
                .unwrap()
                .is_some()
        );
        let repaired_status = engine.pack_status("lottery-analysis").await.unwrap();
        assert!(repaired_status.fingerprint.is_some());
        assert!(repaired_status.last_success_at.is_some());
        assert_eq!(repaired_status.scan_state, "recovered");
        assert!(
            repaired_status
                .scan_history
                .iter()
                .any(|entry| entry.status == "failed")
        );
        assert!(
            repaired_status
                .scan_history
                .iter()
                .any(|entry| entry.status == "succeeded")
        );
        assert_eq!(
            repaired_status
                .last_scan_error
                .as_ref()
                .map(|error| error.code.as_str()),
            Some("source_invalid")
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
