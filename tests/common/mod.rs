//! 集成测试共享辅助（v0_3 / v0_8 等共用）——消除跨测试文件重复。
//!
//! 注意：`http_call` 因各文件签名差异（token 参数）暂未统一，保留在各文件内。

use async_trait::async_trait;
use openaip_os::agent::{AgentEngine, AgentError, Context, Decision};
use serde_json::json;

/// 确定性 Fake 认知引擎：每个认知步产出固定 intent/reasoning/output + 世界对象 refs。
/// v0_3 / v0_8 共用同一实现（v0_9 的简化版行为不同，保留在自身文件）。
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
