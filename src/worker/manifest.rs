//! WorkerManifest —— Worker 的"身份证"（声明能力与执行协议）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 执行协议（四种标准接入方式）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionProtocol {
    /// 进程启动（codex / claude / git / ffmpeg / python）。
    Process,
    /// POST /execute（远程 Agent / 企业服务 / AI Provider）。
    Http,
    /// MCP Server 作为外部能力来源。
    Mcp,
    /// Rust Worker 直接嵌入 Runtime（future）。
    Embedded,
}

/// Worker Manifest。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerManifest {
    pub id: String,
    /// worker 类型：agent / cli / http / mcp / embedded。
    pub worker_type: String,
    /// 提供的能力（Capability 是标准单位，不绑定具体实现）。
    pub capabilities: Vec<String>,
    pub input_schema: Value,
    pub output_schema: Value,
    pub execution: ExecutionProtocol,
    /// 路由评分维度。
    pub cost: u32,
    pub latency_ms: u32,
    pub quality: f64,
    /// 风险等级（1-10；Capability-aware Registry，Phase 11D）。
    pub risk: u32,
    /// 信任度（0..1；Router 评分 = quality × trust）。
    pub trust: f64,
    /// 可用性：local / remote / cloud（候选过滤依据之一）。
    pub availability: String,
}

impl WorkerManifest {
    pub fn new(id: &str, worker_type: &str) -> Self {
        Self {
            id: id.to_string(),
            worker_type: worker_type.to_string(),
            capabilities: Vec::new(),
            input_schema: json_empty(),
            output_schema: json_empty(),
            execution: ExecutionProtocol::Embedded,
            cost: 1,
            latency_ms: 100,
            quality: 0.9,
            risk: 1,
            trust: 1.0,
            availability: "local".to_string(),
        }
    }

    pub fn with_capabilities(mut self, caps: &[&str]) -> Self {
        self.capabilities = caps.iter().map(|c| c.to_string()).collect();
        self
    }

    pub fn with_quality(mut self, q: f64) -> Self {
        self.quality = q;
        self
    }

    pub fn with_risk(mut self, risk: u32) -> Self {
        self.risk = risk;
        self
    }

    pub fn with_trust(mut self, trust: f64) -> Self {
        self.trust = trust;
        self
    }

    pub fn with_availability(mut self, availability: &str) -> Self {
        self.availability = availability.to_string();
        self
    }

    pub fn supports(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|c| c == capability)
    }
}

fn json_empty() -> Value {
    serde_json::json!({})
}
