//! 集成测试共享辅助（v0_3 / v0_7 / v0_8 / v0_9 复用）——消除跨测试文件重复。
//!
//! - `FakeEngine` / `UnavailableEngine`：确定性认知引擎（测试用，不发货）
//! - `retain_registry` / ODL 常量：声明式动作注册表
//! - `seed_and_run` / `spawn_server`：测试环境搭建
//! - `http_call`（无令牌）/ `http_call_auth`（带 Bearer 令牌）：原始 HTTP 调用

use std::net::SocketAddr;
use std::time::Duration;

use async_trait::async_trait;
use openaip_os::action::ActionRegistry;
use openaip_os::agent::{AgentEngine, AgentError, Context, Decision};
use openaip_os::parse_odl;
use openaip_os::planner::{Planner, generate_run_id};
use openaip_os::runtime::RuntimeKernel;
use openaip_os::server;
use openaip_os::storage::LibSQLStorage;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// 辅助库：各测试文件（独立 crate）未必使用全部项，允许死代码。
#[allow(dead_code)]
/// 确定性 Fake 认知引擎：按 step 返回结构化决策，绑定世界对象。
pub struct FakeEngine;

#[async_trait]
impl AgentEngine for FakeEngine {
    fn name(&self) -> &str {
        "fake"
    }
    async fn reason(&self, ctx: &Context) -> Result<Decision, AgentError> {
        let intent = match ctx.step.as_str() {
            "Goal" => "Customer Retention",
            "Intent" => "Reduce churn for at-risk customers",
            "Reasoning" => "Payment delay and order drop signal churn risk",
            "Planning" => "Create retention tasks and link relations",
            _ => ctx.step.as_str(),
        }
        .to_string();
        let refs: Vec<String> = ctx
            .world
            .objects
            .iter()
            .filter_map(|o| o.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
            .collect();
        Ok(Decision {
            intent,
            reasoning: format!("fake reasoning for {}", ctx.step),
            confidence: 0.9,
            object_refs: refs,
            output: json!({ "step": ctx.step }),
        })
    }
}

#[allow(dead_code)]
pub struct UnavailableEngine;

#[async_trait]
impl AgentEngine for UnavailableEngine {
    fn name(&self) -> &str {
        "unavailable"
    }
    async fn reason(&self, _ctx: &Context) -> Result<Decision, AgentError> {
        Err(AgentError::Unavailable(
            "test: cognitive engine down".into(),
        ))
    }
    async fn health(&self) -> bool {
        false
    }
}

#[allow(dead_code)]
pub fn retain_registry() -> ActionRegistry {
    ActionRegistry::from_ir(
        &parse_odl(
            r#"
object Customer:
  properties:
    name: string
    level: string
  actions:
    retain:
      creates:
        - type: RetentionTask
          data:
            title: "客户挽留计划"
            customer: self
          link: needs_retention
"#,
        )
        .expect("retain ODL"),
    )
}

#[allow(dead_code)]
pub async fn seed_and_run(engine: &dyn AgentEngine) -> (RuntimeKernel, String) {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    let run_id = generate_run_id();
    Planner::with_actions(&kernel, retain_registry())
        .run_goal(engine, "reduce churn", "retention-agent", &run_id)
        .await
        .expect("planner 应跑通");
    (kernel, run_id)
}

#[allow(dead_code)]
pub async fn http_call(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let req = match body {
        Some(b) => format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}",
            b.len()
        ),
        None => format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    };
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf)).await;
    let text = String::from_utf8_lossy(&buf);
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or(("", ""));
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let cl = head.lines().find_map(|l| {
        l.to_ascii_lowercase()
            .strip_prefix("content-length: ")
            .and_then(|v| v.trim().parse::<usize>().ok())
    });
    let body = match cl {
        Some(n) => rest.get(..n).unwrap_or(rest).to_string(),
        None => rest.to_string(),
    };
    (status, body)
}

/// 带可选 Bearer 令牌的 HTTP 调用（enforce_auth=true 测试用）。
#[allow(dead_code)]
pub async fn http_call_auth(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&str>,
    token: Option<&str>,
) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let auth_line = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let req = match body {
        Some(b) => format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\n{auth_line}Content-Length: {}\r\n\r\n{b}",
            b.len()
        ),
        None => format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n{auth_line}\r\n"
        ),
    };
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf)).await;
    let text = String::from_utf8_lossy(&buf);
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or(("", ""));
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let cl = head.lines().find_map(|l| {
        l.to_ascii_lowercase()
            .strip_prefix("content-length: ")
            .and_then(|v| v.trim().parse::<usize>().ok())
    });
    let body = match cl {
        Some(n) => rest.get(..n).unwrap_or(rest).to_string(),
        None => rest.to_string(),
    };
    (status, body)
}

#[allow(dead_code)]
pub async fn spawn_server(kernel: std::sync::Arc<RuntimeKernel>) -> SocketAddr {
    let engine: std::sync::Arc<dyn AgentEngine> = std::sync::Arc::new(FakeEngine);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ks = kernel.clone();
    tokio::spawn(async move {
        let _ = server::serve(
            ks,
            engine,
            listener,
            "ui/dist",
            retain_registry(),
            false,
            false,
            false,
        )
        .await;
    });
    tokio::time::sleep(Duration::from_millis(150)).await; // 等绑定就绪
    addr
}

// ── V0.4 完整 Action Engine（REQ-014）───────────────────────────────────────
#[allow(dead_code)]
pub const ODL_ACTIONS: &str = r#"
object Customer:
  properties:
    name: string
    level: string
  actions:
    updateLevel:
      params:
        level: string
      updates:
        - field: level
          value: level
    createOrder:
      params:
        amount: float
      creates:
        - type: Order
          data:
            amount: amount
            customer: self
          link: places
"#;

#[allow(dead_code)]
pub const ODL_RETAIN: &str = r#"
object Customer:
  properties:
    name: string
    level: string
  actions:
    retain:
      creates:
        - type: RetentionTask
          data:
            title: "客户挽留计划"
            customer: self
          link: needs_retention
"#;

// 带 Policy 的动作：退款需人工审批（require_approval），且金额须 <= 1000（precondition）。
#[allow(dead_code)]
pub const ODL_REFUND: &str = r#"
object Customer:
  properties:
    name: string
    level: string
  actions:
    refund:
      params:
        amount: float
      updates:
        - field: balance
          value: amount
      policy:
        require_approval: true
        precondition:
          - field: amount
            op: le
            value: 1000
          - field: self.level
            op: ne
            value: "BLACKLIST"
"#;

// 仅含审批闸门的动作（无前置条件），用于 Planner 闭环验证 Human Approval 门禁。
#[allow(dead_code)]
pub const ODL_APPROVE: &str = r#"
object Customer:
  properties:
    name: string
    level: string
  actions:
    closeAccount:
      policy:
        require_approval: true
"#;

// TC-014a：ODL `actions:` 块解析为 IR（params / updates / creates / data / link / self）
