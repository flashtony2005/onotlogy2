//! PiEngine —— 真实 LLM 认知引擎（M1b · G3，V1 `PiAgentEngine` 的 V2 迁移）。
//!
//! 以 `pi`（`@earendil-works/pi-coding-agent`）为 `CognitiveEngine` 的实现：
//! 经 `--mode text --print --no-session --no-tools --no-context-files` 非交互调用，
//! 禁用文件工具（Pi 仅作认知引擎，禁止直接掌管文件系统）。
//!
//! `decide(view)` → 子进程推理 → 解析 `{capability, args, confidence, reasoning}`：
//! - capability=null/缺省 → `Ok(None)`（无更多动作，D5 完成判定路径）；
//! - 视图外能力 → `Err(UnknownCapability)`（fail-safe，绝不信任引擎越权）；
//! - 缺失/崩溃/超时 → `Err(Engine(...))`（上层映射 WB2004）。
//!
//! `CognitiveEngine::decide` 是同步 trait（Kernel 借用语义），故本实现用
//! `std::process` 阻塞调用 + `try_wait` 轮询实现超时 kill+wait 回收（H1 语义，
//! 不遗留孤儿进程）。pi 输出量小，阻塞可接受；server 侧可整体包 spawn_blocking。

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::engine::{AgentView, CognitiveEngine, CognitiveError, Decision};

/// 以 pi 为认知引擎的实现（V2）。
pub struct PiEngine {
    agent_id: String,
    provider: Option<String>,
    model: Option<String>,
    api_key_env: Option<String>,
    timeout: Duration,
}

impl PiEngine {
    /// 构造引擎。可选通过环境变量指定 LLM Provider（便于接入本地 OpenAI 兼容网关）：
    /// `OPENAIP_PROVIDER` / `OPENAIP_MODEL` / `OPENAIP_API_KEY_ENV`。
    /// 未设置时回退到 pi 内置默认 provider（settings.json 的 defaultProvider）。
    pub fn new(agent_id: impl Into<String>) -> Self {
        let agent_id = agent_id.into();
        let mut s = Self {
            agent_id,
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

    /// 覆盖单次推理超时（默认 120s）。测试注入极短超时验证超时杀进程路径。
    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }

    /// 指定 LLM Provider（默认 pi 内置）。
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

    /// pi 二进制是否可用（WB2004 前置检查同源：PI_BIN 优先，PATH 兜底）。
    pub fn health(&self) -> bool {
        which_pi().is_some()
    }

    /// 组装系统提示：约束 Pi 仅作认知引擎、返回结构化 JSON，并标识当前 Agent。
    fn system_prompt(&self) -> String {
        format!(
            "你是 OpenAIP-OS 的 Cognitive Engine（决策由 OpenAIP Runtime 拥有）。\
             你仅负责基于观察选择下一步执行的能力，禁止直接执行文件/数据库操作。\
             当前 Agent 为 `{agent}`。基于给定的世界观察与可用能力列表，\
             以 JSON 返回：{{\"capability\":<可用能力名或 null>,\"args\":{{...}},\
             \"confidence\":<0~1 float>,\"reasoning\":<str>}}。\
             无更多动作时 capability 返回 null。只允许选择可用能力列表中的名字。",
            agent = self.agent_id
        )
    }

    /// 组装用户提示：目标 + 可用能力 + 相关关系 + 记忆 + 风险信号。
    fn user_prompt(&self, view: &AgentView) -> String {
        let risks: Vec<Value> = view
            .evolution
            .iter()
            .map(|r| {
                json!({
                    "action": r.action,
                    "failed": r.failed,
                    "compensated": r.compensated,
                    "risk_score": r.risk_score,
                })
            })
            .collect();
        format!(
            "目标: {goal}\n\
             可用能力: {caps}\n\
             世界观察（相关关系）: {related}\n\
             历史记忆: {memory}\n\
             风险信号: {risks}\n\
             请选择下一步能力并给出参数。",
            goal = view.goal,
            caps = view.capabilities.join(", "),
            related = serde_json::to_string(&view.related).unwrap_or_else(|_| "[]".into()),
            memory = serde_json::to_string(&view.memory).unwrap_or_else(|_| "[]".into()),
            risks = serde_json::to_string(&risks).unwrap_or_else(|_| "[]".into()),
        )
    }

    /// 同步执行 pi 子进程（阻塞 + try_wait 轮询超时；超时 kill+wait 回收）。
    fn run_pi(&self, system: &str, user: &str) -> Result<String, CognitiveError> {
        let pi_bin = match which_pi() {
            Some(p) => p,
            None => {
                return Err(CognitiveError::Engine(
                    "未找到 `pi` 可执行文件（请先安装 @earendil-works/pi-coding-agent）".into(),
                ));
            }
        };

        let mut cmd = Command::new(&pi_bin);
        cmd.arg("--mode")
            .arg("text")
            .arg("--print")
            .arg("--no-session")
            .arg("--no-tools") // 禁用文件工具：Pi 仅作认知引擎
            .arg("--no-context-files") // 不自动注入项目指令，保持上下文受控
            .arg("--system-prompt")
            .arg(system)
            .arg(user)
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
            .map_err(|e| CognitiveError::Engine(format!("启动 pi 进程失败: {e}")))?;

        // 超时轮询：try_wait 不阻塞；到点 kill + wait 回收（H1 语义）
        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child
                .try_wait()
                .map_err(|e| CognitiveError::Engine(format!("等待 pi 退出失败: {e}")))?
            {
                Some(st) => break st,
                None => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(CognitiveError::Engine("pi 推理超时（已终止进程）".into()));
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        };

        let output = child
            .wait_with_output()
            .map_err(|e| CognitiveError::Engine(format!("读取 pi 输出失败: {e}")))?;
        if !status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CognitiveError::Engine(format!(
                "pi 进程退出非 0：{}",
                stderr.lines().next().unwrap_or("")
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// 解析 pi 的 JSON 输出为 V2 Decision；capability=null → None；视图外能力拒绝。
    fn parse_decision(
        &self,
        raw: &str,
        view: &AgentView,
    ) -> Result<Option<Decision>, CognitiveError> {
        let cleaned = strip_code_fence(raw);
        let parsed: Value = serde_json::from_str(&cleaned).map_err(|e| {
            CognitiveError::Engine(format!("pi 输出解析失败（要求结构化 JSON）: {e}"))
        })?;

        // capability 缺失 / null / 空串 → 无更多动作
        let cap_name = match parsed.get("capability").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s,
            _ => return Ok(None),
        };
        // fail-safe：视图外能力直接拒绝（绝不信任引擎越权）
        if !view.capabilities.iter().any(|c| c == cap_name) {
            return Err(CognitiveError::UnknownCapability(cap_name.to_string()));
        }
        let args = match parsed.get("args").cloned().unwrap_or(json!({})) {
            v @ Value::Object(_) => v,
            _ => json!({}),
        };
        let confidence = parsed
            .get("confidence")
            .and_then(|v| v.as_f64())
            .filter(|c| (0.0..=1.0).contains(c))
            .unwrap_or(0.5);
        Ok(Some(Decision {
            capability: cap_name.to_string(),
            args,
            confidence,
            rule: None,
        }))
    }
}

impl CognitiveEngine for PiEngine {
    fn decide(&self, view: &AgentView) -> Result<Option<Decision>, CognitiveError> {
        let raw = self.run_pi(&self.system_prompt(), &self.user_prompt(view))?;
        self.parse_decision(&raw, view)
    }
}

// ── 共享助手（V1 agent.rs 亦复用；M1d 删除 agent.rs 后归 pi.rs 独有）────────

/// 在 PATH 中查找 `pi` 可执行文件（PI_BIN 环境变量优先）。
/// 环境变量路径必须真实存在（fail-safe：health 不得对悬空路径误报可用）。
pub(crate) fn which_pi() -> Option<String> {
    if let Some(p) = std::env::var("PI_BIN").ok().filter(|p| !p.is_empty()) {
        return std::path::Path::new(&p).is_file().then_some(p);
    }
    // 简单 PATH 探测（避免引入 which crate）。
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join("pi");
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
        // Windows 兼容
        let candidate_exe = dir.join("pi.exe");
        if candidate_exe.is_file() {
            return Some(candidate_exe.to_string_lossy().into_owned());
        }
    }
    None
}

/// 剥离 ```` ```json ... ``` ```` 代码围栏（真实 LLM 常包裹 JSON），便于解析。
pub(crate) fn strip_code_fence(raw: &str) -> String {
    let s = raw.trim();
    if let Some(stripped) = s.strip_prefix("```json").or_else(|| s.strip_prefix("```")) {
        if let Some(end) = stripped.rfind("```") {
            return stripped[..end].trim().to_string();
        }
        return stripped.trim().to_string();
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 围栏剥离（LLM 输出包裹 ```json 的常见形态）。
    #[test]
    fn strip_fence_handles_json_blocks() {
        assert_eq!(strip_code_fence("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fence("plain"), "plain");
    }
}
