use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::oneshot;

use crate::runtime::RuntimeKernel;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::json;

use super::util::lock_mutex;
use super::util::now_micros;
use super::*;

/// REQ-018 自动放行审计器：在 `check_action` 判定自动放行时记录审计。
/// 返回 Err → 调用方必须回退 Human Approval（绝不静默放行）。
#[async_trait]
pub trait ApprovalAuditor: Send + Sync {
    async fn record(&self, learned_id: &str, action: &str) -> Result<(), String>;
}

/// Human Approval 审批器：决定 `require_approval` 的动作是否被放行。
/// 默认 `AutoApprover { allow: false }`（拒，等待人工）；服务/测试可注入自动批准。
///
/// `approve` 为 `async`：CLI 的 `ConsoleApprover` 同步读 stdin 立即返回；
/// 服务的 `ServerApprover` 在此挂起运行、等待 HTTP 端点裁决后恢复。
#[async_trait]
pub trait PolicyApprover: Send + Sync {
    /// 返回 true 表示该步已获人工/自动批准，可越过审批闸门执行。
    async fn approve(&self, step: &PlanStep) -> bool;
}

/// 自动审批器：无人工场景（测试 / 服务）使用；`allow=true` 直接放行。
#[derive(Debug, Clone, Copy)]
pub struct AutoApprover {
    pub allow: bool,
}
#[async_trait]
impl PolicyApprover for AutoApprover {
    async fn approve(&self, _step: &PlanStep) -> bool {
        self.allow
    }
}

/// 交互式审批器：经 stdin 询问 y/n（CLI `agent run` 默认使用）。
///
/// 同一动作（按 `action@target_id`）只询问一次（决策缓存），避免对多个同类目标
/// 重复打断用户。非 Action 步用 description 作 key。
pub struct ConsoleApprover {
    cache: std::sync::Mutex<HashMap<String, bool>>,
}
impl ConsoleApprover {
    pub fn new() -> Self {
        Self {
            cache: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// 取审批 key：声明式动作用 `action@target_id`，其余用 description。
    fn key_of(step: &PlanStep) -> String {
        match &step.capability {
            Capability::Action {
                action, target_id, ..
            } => format!("{action}@{target_id}"),
            _ => step.description.clone(),
        }
    }
}
impl Default for ConsoleApprover {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl PolicyApprover for ConsoleApprover {
    async fn approve(&self, step: &PlanStep) -> bool {
        let key = Self::key_of(step);
        if let Some(v) = lock_mutex(&self.cache).get(&key) {
            return *v;
        }
        let ans = prompt(&format!(
            "⚠️ 需 Human Approval：{}？[y/N] ",
            step.description
        ))
        .await;
        lock_mutex(&self.cache).insert(key, ans);
        ans
    }
}

/// 向 stdout 打印提示并从 stdin 读一行，解析 y/yes（忽略大小写）为批准。
/// `approve` 在 async 上下文调用本函数：经 `spawn_blocking` 读 stdin，不阻塞 event loop，
/// 且不依赖 `block_in_place`（后者仅在多线程 runtime 可用，current_thread runtime 会 panic）。
async fn prompt(question: &str) -> bool {
    print!("{question}");
    let _ = io::stdout().flush();
    let buf = tokio::task::spawn_blocking(move || {
        let mut buf = String::new();
        let _ = io::stdin().read_line(&mut buf);
        buf
    })
    .await
    .unwrap_or_default();
    matches!(buf.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// REQ-018 运行级审计器：把自动放行审计写入内核存储（绑定当前 run_id）。
/// 写库失败 → 返回 Err，使 `check_action` 回退 Human Approval（绝不静默放行）。
pub struct RunAuditor<'a> {
    kernel: &'a RuntimeKernel,
    run_id: String,
}
impl<'a> RunAuditor<'a> {
    pub fn new(kernel: &'a RuntimeKernel, run_id: &str) -> Self {
        Self {
            kernel,
            run_id: run_id.to_string(),
        }
    }
}
#[async_trait]
impl ApprovalAuditor for RunAuditor<'_> {
    async fn record(&self, learned_id: &str, action: &str) -> Result<(), String> {
        self.kernel
            .record_approval_audit(
                &self.run_id,
                action,
                learned_id,
                "auto_approve",
                "5 道闸门全过，替代一次 Human Approval",
            )
            .await
            .map_err(|e| e.to_string())
    }
}

/// 待审批请求状态（与 timeline / HTTP 响应对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum ApprovalState {
    Pending,
    Approved,
    Rejected,
    /// 挂起超时（Planner 已按拒绝回退）：与人工拒绝（Rejected）区分，UI 不再显示幽灵待审批。
    Expired,
}

impl ApprovalState {
    /// 状态 → 持久化字符串。
    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalState::Pending => "Pending",
            ApprovalState::Approved => "Approved",
            ApprovalState::Rejected => "Rejected",
            ApprovalState::Expired => "Expired",
        }
    }

    /// 持久化字符串 → 状态（未知 → Pending，安全兜底）。
    pub fn from_db_str(s: &str) -> ApprovalState {
        match s {
            "Approved" => ApprovalState::Approved,
            "Rejected" => ApprovalState::Rejected,
            "Expired" => ApprovalState::Expired,
            _ => ApprovalState::Pending,
        }
    }
}

/// 一条待审批请求（挂起态，供浏览器/HTTP 审批端点裁决后恢复运行）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub req_id: String,
    pub run_id: String,
    pub agent_id: String,
    /// 动作标识（`action@target_id`），便于 UI 展示与去重。
    pub step_id: String,
    pub description: String,
    pub state: ApprovalState,
}

/// 审批请求存储：连接 Planner 挂起点与 HTTP 审批端点（V0.4 遗留的 server 挂起式 HTTP 审批）。
pub struct ApprovalStore {
    requests: Mutex<HashMap<String, ApprovalEntry>>,
}

struct ApprovalEntry {
    request: ApprovalRequest,
    /// 挂起 Planner 的唤醒信道；裁决后置 None。
    tx: Option<oneshot::Sender<bool>>,
}

impl ApprovalStore {
    pub fn new() -> Self {
        Self {
            requests: Mutex::new(HashMap::new()),
        }
    }

    /// 列出某运行的所有审批请求（含历史裁决），供 UI 渲染审批列表。
    pub fn list_for_run(&self, run_id: &str) -> Vec<ApprovalRequest> {
        self.requests
            .lock()
            .unwrap()
            .values()
            .filter(|e| e.request.run_id == run_id)
            .map(|e| e.request.clone())
            .collect()
    }

    /// 取单条审批请求（快照）。
    pub fn get(&self, req_id: &str) -> Option<ApprovalRequest> {
        self.requests
            .lock()
            .unwrap()
            .get(req_id)
            .map(|e| e.request.clone())
    }

    /// 内部：注册待审批并挂起（由 `ServerApprover` 调用，返回裁决信道接收端）。
    fn register(&self, req: ApprovalRequest) -> oneshot::Receiver<bool> {
        let (tx, rx) = oneshot::channel();
        lock_mutex(&self.requests).insert(
            req.req_id.clone(),
            ApprovalEntry {
                request: req,
                tx: Some(tx),
            },
        );
        rx
    }

    /// 超时回收：挂起 Planner 已按拒绝回退后调用，把残留请求翻为 Expired 并清掉 tx。
    /// 之后 `resolve` 对其返回 false（映射 404），避免「请求仍在 Pending + resolve 误判成功」
    /// 导致的 UI 幽灵待审批（Critical-1/2）；Expired 与人工 Rejected 可区分（Critical-3）。
    /// 已裁决（tx 为 None）或已 Expired 的请求幂等：直接返回。
    pub fn expire(&self, req_id: &str) {
        let mut g = lock_mutex(&self.requests);
        if let Some(e) = g.get_mut(req_id) {
            e.tx = None;
            if e.request.state == ApprovalState::Pending {
                e.request.state = ApprovalState::Expired;
            }
        }
    }

    /// 测试专用：直接插入一条请求（绕过挂起信道生命周期），便于单测 resolve/expire 语义。
    /// 非 cfg 限定，因集成测试位于独立 crate，无法访问 cfg(test) 项。
    #[doc(hidden)]
    pub fn requests_insert_for_test(
        &self,
        req_id: &str,
        state: ApprovalState,
        tx: Option<oneshot::Sender<bool>>,
    ) {
        use crate::planner::ApprovalRequest;
        lock_mutex(&self.requests).insert(
            req_id.to_string(),
            ApprovalEntry {
                request: ApprovalRequest {
                    req_id: req_id.to_string(),
                    run_id: "test-run".to_string(),
                    agent_id: "test-agent".to_string(),
                    step_id: "step".to_string(),
                    description: "test".to_string(),
                    state,
                },
                tx,
            },
        );
    }

    /// 裁决：发信号唤醒挂起的 Planner，更新状态；返回是否成功裁决。
    /// 请求不存在 / 已裁决（tx 已被取走）/ 已超时 Expired → 返回 false（HTTP 端点映射 404）。
    pub fn resolve(&self, req_id: &str, approve: bool) -> bool {
        let mut g = lock_mutex(&self.requests);
        let Some(e) = g.get_mut(req_id) else {
            return false;
        };
        // 已裁决（tx 已被取走）或已超时（Expired）→ 拒绝裁决
        let Some(tx) = e.tx.take() else {
            return false;
        };
        e.request.state = if approve {
            ApprovalState::Approved
        } else {
            ApprovalState::Rejected
        };
        let _ = tx.send(approve);
        true
    }
}

impl Default for ApprovalStore {
    fn default() -> Self {
        Self::new()
    }
}

/// 服务端挂起式审批器：遇 `require_approval` 动作时挂起运行，注册待审批请求，
/// 并发 `PendingApproval` 认知事件（含 `reqId`）；浏览器经 HTTP 端点裁决后恢复。
/// 对应 V0.4 遗留的 server 挂起式 HTTP 审批端点（让浏览器点批准）。
pub struct ServerApprover {
    kernel: Arc<RuntimeKernel>,
    store: Arc<ApprovalStore>,
    run_id: String,
    agent_id: String,
    seq: AtomicU64,
    /// 挂起超时（默认 APPROVAL_TIMEOUT）；测试可注入极短超时触发超时回收路径。
    timeout: Duration,
}

impl ServerApprover {
    pub fn new(
        kernel: Arc<RuntimeKernel>,
        store: Arc<ApprovalStore>,
        run_id: String,
        agent_id: String,
    ) -> Self {
        Self {
            kernel,
            store,
            run_id,
            agent_id,
            seq: AtomicU64::new(0),
            timeout: APPROVAL_TIMEOUT,
        }
    }

    /// 覆盖挂起超时（默认 APPROVAL_TIMEOUT）；测试注入极短超时触发 Expired 回收路径。
    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }
}

#[async_trait]
impl PolicyApprover for ServerApprover {
    async fn approve(&self, step: &PlanStep) -> bool {
        let req_id = format!(
            "apr_{}_{}",
            self.run_id,
            self.seq.fetch_add(1, Ordering::SeqCst)
        );
        let step_id = ConsoleApprover::key_of(step);
        // 注册待审批并挂起（rx 在 resolve 时被唤醒）
        let rx = self.store.register(ApprovalRequest {
            req_id: req_id.clone(),
            run_id: self.run_id.clone(),
            agent_id: self.agent_id.clone(),
            step_id: step_id.clone(),
            description: step.description.clone(),
            state: ApprovalState::Pending,
        });
        // 持久化待审批（DB 真相源；内存 store 仅服务 oneshot 唤醒，重启后虽无法唤醒但不影响审计/列表）
        let _ = self
            .kernel
            .storage()
            .insert_approval(
                &req_id,
                &self.run_id,
                &self.agent_id,
                &step_id,
                &step.description,
            )
            .await;
        // 发 PendingApproval 认知事件（含 reqId，UI 据此渲染审批按钮）
        let _ = self
            .kernel
            .emit_cognitive(
                "PendingApproval",
                &self.run_id,
                &self.agent_id,
                &step.object_refs,
                Some(&format!("等待人工审批: {}", step.description)),
                Some(
                    json!({ "reqId": req_id, "stepId": step_id, "description": step.description }),
                ),
                Some(json!("Pending")),
                Some(0.0),
            )
            .await;
        // 挂起等待 HTTP 端点裁决（approve=true 放行 / false=WB2005 终止）。
        // 超时：回收残留请求（store.expire 翻 Expired + 清 tx），避免 UI 幽灵待审批。
        let (decision, outcome) = match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(v)) => (v, if v { "Approved" } else { "Rejected" }),
            _ => {
                self.store.expire(&req_id);
                // M2：超时状态同步落库（DB 真相源），避免重启后残留 Pending 幽灵待审批。
                let _ = self
                    .kernel
                    .storage()
                    .update_approval_state(&req_id, "Expired", None, Some(&now_micros()))
                    .await;
                (false, "Expired")
            }
        };
        // 发 ApprovalDecision 事件（供 timeline / WS 实时反映结果）；
        // Expired 与人工 Rejected 通过 outcome 区分，便于 UI/审计溯源（Critical-3）。
        let _ = self
            .kernel
            .emit_cognitive(
                "ApprovalDecision",
                &self.run_id,
                &self.agent_id,
                &step.object_refs,
                Some(&format!(
                    "审批结果: {}（reqId={}）",
                    match outcome {
                        "Approved" => "批准",
                        "Rejected" => "拒绝",
                        _ => "超时未裁决",
                    },
                    req_id
                )),
                Some(json!({
                    "reqId": req_id,
                    "decision": outcome
                })),
                Some(json!(outcome)),
                Some(if decision { 0.9 } else { 0.0 }),
            )
            .await;
        decision
    }
}
