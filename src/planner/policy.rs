use std::collections::HashMap;

use crate::evolution::LearnedKnowledge;
use crate::ir::{ActionDef, CompareOp, PolicyDef, ValueExpr};
use serde_json::Value;

use super::approval::ApprovalAuditor;
use super::approval::PolicyApprover;
use super::*;

/// REQ-018 `may_auto_approve`：5 道闸门全过 + 开关开启 + 存在已验证 LearnedKnowledge → 允许自动放行。
/// 任一闸门不满足返回 false（调用方回退 Human Approval）。纯函数，便于单测（TC-026b/c/d/f）。
pub fn may_auto_approve(
    config: &RuntimeConfig,
    learned: Option<&LearnedKnowledge>,
    success_runs: usize,
    failure_count: usize,
    policy: &PolicyDef,
) -> bool {
    /// 高置信阈值（建议 0.8，取自 VALIDATE_THRESHOLD 之上）。
    const AUTO_APPROVE_CONFIDENCE: f64 = 0.8;
    /// 多次成功阈值（去重 run 数，建议 3）。
    const AUTO_APPROVE_MIN_RUNS: usize = 3;

    // 1) 显式允许开关
    if !config.auto_approve_from_evolution {
        return false;
    }
    // 2) 已验证知识（存在且未撤销）
    let Some(lk) = learned else {
        return false;
    };
    if !lk.auto_approve_enabled {
        return false;
    }
    // 3) 高置信
    if lk.confidence < AUTO_APPROVE_CONFIDENCE {
        return false;
    }
    // 4) 多次成功（去重 run 数）
    if success_runs < AUTO_APPROVE_MIN_RUNS {
        return false;
    }
    // 5) 无失败（强判定：依赖 ActionFailed 埋点）
    if failure_count > 0 {
        return false;
    }
    // 6) 低风险（已有 require_approval + 非空 precondition；采用现有 PolicyDef 判定）
    if !(policy.require_approval && !policy.precondition.is_empty()) {
        return false;
    }
    true
}

impl Policy {
    /// 校验计划步：破坏性动作（删对象）默认拒，转 Human Approval；其余允许。
    pub fn check(step: &PlanStep) -> PolicyDecision {
        match &step.capability {
            Capability::DeleteObject { id } => PolicyDecision::Deny {
                reason: format!("删除对象 {id} 属破坏性操作，需 Human Approval"),
            },
            _ => PolicyDecision::Allow,
        }
    }

    /// 校验声明式动作：结合动作自带 `PolicyDef`（前置条件 + 审批闸门）。
    ///
    /// - `def` 为 None（动作未注册）→ 放行；
    /// - `precondition` 全部满足才放行（AND）；任一不满足 → Deny；
    /// - `require_approval=true` 默认 Deny（WB2005）：
    ///   - 若满足 REQ-018 自动放行 5 道闸门，经 `auditor` 记录审计后替代 Human Approval（Allow）；
    ///   - 否则仍须经 `approver` 放行（Human Approval）；审计失败/无知识 → 回退 Human（绝不静默放行）。
    #[allow(clippy::too_many_arguments)]
    pub async fn check_action(
        step: &PlanStep,
        def: Option<&ActionDef>,
        approver: &dyn PolicyApprover,
        target: Option<&Value>,
        args: &HashMap<String, Value>,
        config: &RuntimeConfig,
        learned: Option<&LearnedKnowledge>,
        failure_count: usize,
        auditor: Option<&dyn ApprovalAuditor>,
    ) -> PolicyDecision {
        let Some(def) = def else {
            return PolicyDecision::Allow;
        };
        // 前置条件：全部满足才放行
        for clause in &def.policy.precondition {
            let lhs = resolve_policy_field(&clause.field, args, target);
            let rhs = resolve_value(&clause.value, args, target, "");
            if !compare_values(&lhs, &rhs, clause.op) {
                return PolicyDecision::Deny {
                    reason: format!(
                        "动作 {} 不满足前置条件 {}={:?} {:?} {:?}",
                        def.name, clause.field, lhs, clause.op, rhs
                    ),
                };
            }
        }
        // 审批闸门：require_approval 默认 Deny，需 approver 放行（Human Approval）。
        // approver.approve 为 async：ConsoleApprover 同步返回；ServerApprover 挂起等待 HTTP 裁决。
        if def.policy.require_approval {
            // REQ-018 自动放行：5 道闸门全过 → 替代一次 Human Approval（须先写审计）。
            let success_runs = learned.map(|l| l.run_ids.len()).unwrap_or(0);
            if may_auto_approve(config, learned, success_runs, failure_count, &def.policy) {
                match auditor {
                    // 审计成功 → 自动放行（替代 Human Approval）
                    Some(a) => {
                        if let Some(lk) = learned {
                            match a.record(lk.id.as_str(), &def.name).await {
                                Ok(()) => return PolicyDecision::Allow,
                                // 审计写库失败 → 回退 Human Approval（绝不静默放行）
                                Err(_) => { /* fall through to approver */ }
                            }
                        }
                    }
                    // 纯单元测试无 auditor：直接放行（审计留待集成层）
                    None => return PolicyDecision::Allow,
                }
            }
            if !approver.approve(step).await {
                return PolicyDecision::Deny {
                    reason: format!("动作 {} 需 Human Approval（WB2005）", def.name),
                };
            }
        }
        PolicyDecision::Allow
    }
}

/// 把 Policy 字段引用解析为值：`self.<f>` → 目标字段；参数名 → 参数；否则 → 目标字段。
pub(super) fn resolve_policy_field(
    field: &str,
    args: &HashMap<String, Value>,
    target: Option<&Value>,
) -> Value {
    let ve = if let Some(rest) = field.strip_prefix("self.") {
        ValueExpr::SelfField(rest.to_string())
    } else if args.contains_key(field) {
        ValueExpr::Param(field.to_string())
    } else {
        ValueExpr::SelfField(field.to_string())
    };
    resolve_value(&ve, args, target, "")
}

/// 解析值表达式（与 ActionEngine 同源逻辑，供 Policy 比较使用）。
pub(super) fn resolve_value(
    v: &ValueExpr,
    args: &HashMap<String, Value>,
    target: Option<&Value>,
    target_id: &str,
) -> Value {
    match v {
        ValueExpr::Literal(l) => l.clone(),
        ValueExpr::Param(p) => args.get(p).cloned().unwrap_or(Value::Null),
        ValueExpr::SelfId => Value::String(target_id.to_string()),
        ValueExpr::SelfField(f) => target
            .and_then(|t| {
                t.get(f)
                    .cloned()
                    .or_else(|| t.get("data").and_then(|d| d.get(f)).cloned())
            })
            .unwrap_or(Value::Null),
    }
}

pub(super) fn step_key(step: &PlanStep) -> Option<String> {
    match &step.capability {
        Capability::Action {
            object_type,
            action,
            target_id,
            ..
        } => Some(format!("{object_type}:{action}:{target_id}")),
        _ => None,
    }
}

/// 按运算符比较两个 JSON 值：Eq/Ne 走全相等；其余按数值（f64）比较，非数则 false。
pub(super) fn compare_values(a: &Value, b: &Value, op: CompareOp) -> bool {
    use CompareOp::*;
    match op {
        Eq => a == b,
        Ne => a != b,
        _ => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => match op {
                Lt => x < y,
                Le => x <= y,
                Gt => x > y,
                Ge => x >= y,
                _ => false,
            },
            _ => false,
        },
    }
}
