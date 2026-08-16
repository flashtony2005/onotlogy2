//! 认知引擎接入层（对应 REQ-010 / prd-agent-integration.md / module-split.md 4.3）
//!
//! Pi 仅作 **Cognitive Engine**，经 `AgentEngine` trait 接入；OpenAIP 拥有 Planner / Policy /
//! Ontology / World Model / Trace。引擎可替换：`PiAgentEngine`（MVP）未来可换
//! `ClaudeAgentEngine` / `QwenAgentEngine` / `LocalLLMEngine`，Planner 不依赖其内部结构。
//!
//! [强制] 引擎任何不可用（pi 未安装 / 进程崩溃 / LLM 鉴权失败 / 超时）→ 返 `AgentError::Unavailable`
//! → 上层映射 WB2004，运行安全失败（见 TC-010-b）。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;

// M1b G3：pi 探测与围栏剥离已迁至 V2 认知引擎（cognitive/pi.rs），
// V1 PiAgentEngine 复用同一实现；M1d 删除本文件后归 pi.rs 独有。
use crate::cognitive::pi::{strip_code_fence, which_pi};

/// 认知引擎错误。
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// 认知引擎不可用（Pi 未就绪 / 崩溃 / LLM 鉴权失败 / 超时）→ 映射 WB2004。
    #[error("认知引擎不可用（Pi 未就绪/崩溃/鉴权失败）: {0}")]
    Unavailable(String),
    /// 引擎输出无法解析为结构化决策。
    #[error("认知引擎输出解析失败: {0}")]
    Parse(String),
}

/// 世界快照：认知引擎仅基于此最小必要快照推理（prd 4.4：上下文裁剪，避免体积过大）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorldSnapshot {
    /// 对象摘要（含 id / type / data）。
    pub objects: Vec<Value>,
    /// 关系摘要（含 source / relation / target）。
    pub relations: Vec<Value>,
}

/// 认知步输入上下文（Planner 在链路各步构造并传给引擎）。
#[derive(Debug, Clone)]
pub struct Context {
    /// 自然语言目标。
    pub goal: String,
    /// 本次运行 ID。
    pub run_id: String,
    /// Agent ID。
    pub agent_id: String,
    /// 当前认知步类型（Goal / Intent / OntologyQuery / Reasoning / Planning / ...），用于引擎路由。
    pub step: String,
    /// 上一步的结构化输出，供链路传递。
    pub prior_output: Value,
    /// 世界模型快照。
    pub world: WorldSnapshot,
}

/// 认知决策：引擎对每个步给出的结果，Planner 据此构造 cognitive `TraceEvent`。
#[derive(Debug, Clone)]
pub struct Decision {
    /// 意图（如 "Customer Retention"）。
    pub intent: String,
    /// 推理过程（可解释性根基，映射到 TraceEvent.reasoning）。
    pub reasoning: String,
    /// 置信度 0~1（低于阈值默认 0.5 标记需 Human Approval，见 prd 4.4）。
    pub confidence: f64,
    /// 绑定 Ontology 对象（可解释性根基；映射到 object_refs）。
    pub object_refs: Vec<String>,
    /// 结构化输出（如解析后的 Goal / 生成的 PlanStep 列表）。
    pub output: Value,
}

/// 可替换认知引擎（Cognitive Engine）。Pi 经此 trait 接入；未来可换其它引擎。
#[async_trait]
pub trait AgentEngine: Send + Sync {
    /// 引擎名称（如 "pi"）。
    fn name(&self) -> &str;
    /// 基于上下文推理，返回结构化决策。
    async fn reason(&self, ctx: &Context) -> Result<Decision, AgentError>;
    /// 引擎是否可用（默认 true）。`PiAgentEngine` 覆写为探测 `pi` 二进制是否存在，
    /// 供 `POST /run` 快速判定 WB2004，避免悬挂连接（pi-doc / prd 4.5）。
    async fn health(&self) -> bool {
        true
    }
}

/// 以 `pi`（`@earendil-works/pi-coding-agent`）为认知引擎的实现（MVP 唯一生产引擎）。
///
/// 通过 `--mode text --print --no-session --no-tools` 非交互调用，禁用其文件工具
/// （prd 3：Pi 禁止直接掌管文件系统），仅承担 NLU / 推理。stdout 直接为模型文本
/// （我们要求其返回 JSON），`parse_decision` 解析为结构化 `Decision`。任何失败 →
/// `Unavailable`（WB2004）。
pub struct PiAgentEngine {
    agent_id: String,
    provider: Option<String>,
    model: Option<String>,
    api_key_env: Option<String>,
    timeout: Duration,
}

impl PiAgentEngine {
    /// 构造引擎。可选通过环境变量指定 LLM Provider（便于接入本地 OpenAI 兼容网关，
    /// 如 cc switch）：`OPENAIP_PROVIDER` / `OPENAIP_MODEL` / `OPENAIP_API_KEY_ENV`。
    /// 未设置时回退到 pi 内置默认 provider（settings.json 的 defaultProvider）。
    pub fn new(agent_id: impl Into<String>) -> Self {
        let agent_id = agent_id.into();
        let mut s = Self {
            agent_id: agent_id.clone(),
            provider: None,
            model: None,
            api_key_env: None,
            timeout: Duration::from_secs(120),
        };
        if let Ok(p) = std::env::var("OPENAIP_PROVIDER")
            && !p.is_empty()
        {
            s.provider = Some(p);
        }
        if let Ok(m) = std::env::var("OPENAIP_MODEL")
            && !m.is_empty()
        {
            s.model = Some(m);
        }
        if let Ok(k) = std::env::var("OPENAIP_API_KEY_ENV")
            && !k.is_empty()
        {
            s.api_key_env = Some(k);
        }
        s
    }

    /// 覆盖单次推理超时（默认 120s）。测试注入极短超时验证超时杀进程路径（H1）。
    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }

    /// 指定 LLM Provider（默认 google，经 pi 内置）。
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// 指定模型（支持 "provider/id" 形式）。
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// 从指定环境变量读取 API Key（默认走 pi 内置 env 解析）。
    pub fn with_api_key_env(mut self, env: impl Into<String>) -> Self {
        self.api_key_env = Some(env.into());
        self
    }

    /// 组装系统提示：约束 Pi 仅作认知引擎、返回结构化 JSON，并标识当前 Agent。
    fn system_prompt(&self) -> String {
        format!(
            "你是 OpenAIP-OS 的 Cognitive Engine（决策由 OpenAIP Planner 拥有）。\
             你仅负责 NLU 与推理，禁止直接执行文件/数据库操作。\
             当前 Agent 为 `{agent}`。针对给定认知步与目标，基于提供的世界模型快照推理，\
             并以 JSON 返回：{{\"intent\":<str>,\"reasoning\":<str>,\"confidence\":<0~1 float>,\
             \"object_refs\":[<对象id>],\"output\":<任意 JSON>}}。",
            agent = self.agent_id
        )
    }

    /// 组装用户提示：步类型 + 目标 + 上一步输出 + 世界快照。
    fn user_prompt(&self, ctx: &Context) -> String {
        let world = serde_json::to_string(&ctx.world).unwrap_or_else(|_| "{}".to_string());
        format!(
            "认知步: {step}\n目标: {goal}\n上一步输出: {prior}\n世界模型快照: {world}\n\
             请基于快照给出本步决策（仅引用快照中存在的对象 id 作为 object_refs）。",
            step = ctx.step,
            goal = ctx.goal,
            prior = ctx.prior_output,
            world = world
        )
    }

    /// 解析 pi 的 JSON 输出为 `Decision`；解析失败则 best-effort 兜底（不致命）。
    fn parse_decision(&self, raw: &str, ctx: &Context) -> Decision {
        // 优先整体解析为 JSON（剥离 ```json 围栏后）；否则在输出中尽力提取。
        let cleaned = strip_code_fence(raw);
        let parsed: Value = match serde_json::from_str(&cleaned) {
            Ok(v) => v,
            Err(_) => {
                // 模型可能未严格返回 JSON；尽力下钻取文本。
                let text = extract_text(raw);
                return Decision {
                    intent: ctx.step.clone(),
                    reasoning: text,
                    confidence: 0.5,
                    object_refs: collect_refs(ctx),
                    output: Value::Null,
                };
            }
        };
        let text = extract_text_from_value(&parsed);
        Decision {
            intent: parsed
                .get("intent")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| ctx.step.clone()),
            reasoning: text,
            confidence: parsed
                .get("confidence")
                .and_then(|v| v.as_f64())
                .filter(|c| (0.0..=1.0).contains(c))
                .unwrap_or(0.5),
            object_refs: parsed
                .get("object_refs")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_else(|| collect_refs(ctx)),
            output: parsed.get("output").cloned().unwrap_or(Value::Null),
        }
    }
}

#[async_trait]
impl AgentEngine for PiAgentEngine {
    fn name(&self) -> &str {
        "pi"
    }

    async fn health(&self) -> bool {
        which_pi().is_some()
    }

    async fn reason(&self, ctx: &Context) -> Result<Decision, AgentError> {
        // 探测 pi 是否可用（未安装 → 直接 Unavailable，避免无效 spawn）。
        let pi_bin = match which_pi() {
            Some(p) => p,
            None => {
                return Err(AgentError::Unavailable(
                    "未找到 `pi` 可执行文件（请先安装 @earendil-works/pi-coding-agent）".into(),
                ));
            }
        };

        let mut cmd = tokio::process::Command::new(&pi_bin);
        cmd.arg("--mode")
            .arg("text")
            .arg("--print")
            .arg("--no-session")
            .arg("--no-tools") // 禁用文件工具：Pi 仅作认知引擎
            .arg("--no-context-files") // 不自动注入项目指令，保持上下文受控
            .arg("--system-prompt")
            .arg(self.system_prompt())
            .arg(self.user_prompt(ctx))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(p) = &self.provider {
            cmd.arg("--provider").arg(p);
        }
        if let Some(m) = &self.model {
            cmd.arg("--model").arg(m);
        }
        if let Some(key) = self
            .api_key_env
            .as_ref()
            .and_then(|e| std::env::var(e).ok())
        {
            cmd.arg("--api-key").arg(key);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| AgentError::Unavailable(format!("启动 pi 进程失败: {e}")))?;

        // H1：超时必须杀进程并回收，避免 pi 成孤儿继续消耗 LLM 额度/资源。
        // `wait` 借用 child（不消费），超时分支可继续 kill + 回收；输出在进程结束后
        // 从管道读取（pi 决策输出量小，不会触发管道满阻塞；若真阻塞会命中超时兜底）。
        let status = match tokio::time::timeout(self.timeout, child.wait()).await {
            Ok(res) => {
                res.map_err(|e| AgentError::Unavailable(format!("等待 pi 退出失败: {e}")))?
            }
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await; // 回收僵尸进程
                return Err(AgentError::Unavailable("pi 推理超时（已终止进程）".into()));
            }
        };
        let mut stdout = String::new();
        let mut stderr = String::new();
        if let Some(mut s) = child.stdout.take() {
            let _ = s.read_to_string(&mut stdout).await;
        }
        if let Some(mut s) = child.stderr.take() {
            let _ = s.read_to_string(&mut stderr).await;
        }

        if !status.success() {
            return Err(AgentError::Unavailable(format!(
                "pi 进程退出非 0：{}",
                stderr.lines().next().unwrap_or("")
            )));
        }

        Ok(self.parse_decision(stdout.trim(), ctx))
    }
}

/// 从 pi 文本输出中尽力提取可读文本（剥离 JSON 包裹）。
fn extract_text(raw: &str) -> String {
    match serde_json::from_str::<Value>(raw) {
        Ok(v) => extract_text_from_value(&v),
        Err(_) => raw.trim().to_string(),
    }
}

/// 从解析后的 JSON 值下钻提取推理文本（兼容 {message}/{content}/裸字符串）。
fn extract_text_from_value(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    for key in ["reasoning", "content", "message", "text"] {
        if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
            return s.to_string();
        }
    }
    if let Some(arr) = v.get("messages").and_then(|x| x.as_array()) {
        let mut out = String::new();
        for m in arr {
            if let Some(s) = m.get("content").and_then(|x| x.as_str()) {
                out.push_str(s);
                out.push('\n');
            }
        }
        if !out.is_empty() {
            return out.trim().to_string();
        }
    }
    serde_json::to_string(v).unwrap_or_default()
}

/// 从世界快照收集全部对象 id（兜底 object_refs）。
fn collect_refs(ctx: &Context) -> Vec<String> {
    ctx.world
        .objects
        .iter()
        .filter_map(|o| o.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// H1 回归：推理超时必须 kill 子进程并回收，不得遗留孤儿 pi 进程。
    #[tokio::test]
    async fn timeout_kills_child_process() {
        let dir = std::env::temp_dir();
        let script = dir.join(format!("fake_pi_{}.sh", std::process::id()));
        let pidfile = dir.join(format!("fake_pi_{}.pid", std::process::id()));
        let _ = std::fs::remove_file(&pidfile);
        // 假 pi：记录自身 PID 后长睡（超过注入的超时）
        std::fs::write(
            &script,
            format!("#!/bin/sh\necho $$ > {}\nsleep 30\n", pidfile.display()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // edition 2024：set_var 为 unsafe
        unsafe { std::env::set_var("PI_BIN", &script) };
        let engine = PiAgentEngine::new("test-agent").with_timeout(Duration::from_millis(1000));
        let ctx = Context {
            goal: "g".into(),
            run_id: "r".into(),
            agent_id: "a".into(),
            step: "Goal".into(),
            prior_output: Value::Null,
            world: WorldSnapshot::default(),
        };
        let res = engine.reason(&ctx).await;
        unsafe { std::env::remove_var("PI_BIN") };
        let _ = std::fs::remove_file(&script);

        assert!(
            matches!(&res, Err(AgentError::Unavailable(m)) if m.contains("超时")),
            "应返回超时错误，实际: {res:?}"
        );
        // 进程应已被 kill + wait 回收：kill -0 应失败（进程不存在）
        let pid = std::fs::read_to_string(&pidfile).unwrap_or_default();
        let _ = std::fs::remove_file(&pidfile);
        let pid = pid.trim();
        assert!(!pid.is_empty(), "假 pi 应已启动并记录 PID");
        let alive = std::process::Command::new("kill")
            .args(["-0", pid])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(!alive, "H1：超时后 pi 子进程（pid={pid}）应已被终止");
    }
}
