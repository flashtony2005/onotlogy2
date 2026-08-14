//! Policy —— 组合层约束（对 Capability / Effect 的声明式规则）。
//!
//! 不是 Kernel Primitive：Policy 是「在什么条件下允许提交 Effect」的约束。
//! - precondition：args 字段比较（AND）
//! - require_approval：Effect 提交前挂起等待 `Approval` 关系（Phase 1 的 WAITING + relate 恢复）
//!
//! Deny 语义：业务拒绝（Effect 未提交），不是运行失败——不触发补偿。

use serde_json::Value;

/// 字段比较操作符。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// 前置条件：`args[param] <op> value`。
#[derive(Debug, Clone)]
pub struct Precondition {
    pub param: String,
    pub op: Cmp,
    pub value: Value,
}

/// 动作级策略。
#[derive(Debug, Clone, Default)]
pub struct Policy {
    /// 提交前需人工审批（Fiber 挂起等待 `Approval` 关系）。
    pub require_approval: bool,
    /// 前置条件（AND，全部满足才 Allow）。
    pub preconditions: Vec<Precondition>,
}

/// 策略判定。
#[derive(Debug, Clone, PartialEq)]
pub enum PolicyVerdict {
    Allow,
    /// 业务拒绝（Effect 未提交；Fiber 走失败路径，无 Effect 故补偿链为空）。
    Deny(String),
    /// 需人工审批：调用方应 wait_on(Approval) + advance(Waiting)。
    RequireApproval,
}

impl Policy {
    pub fn check(&self, args: &Value) -> PolicyVerdict {
        for p in &self.preconditions {
            let got = args.get(&p.param).unwrap_or(&Value::Null);
            if !compare(got, &p.value, p.op) {
                return PolicyVerdict::Deny(format!(
                    "前置条件不满足: {} {} {:?}（实际 {:?}）",
                    p.param,
                    cmp_str(p.op),
                    p.value,
                    got
                ));
            }
        }
        if self.require_approval {
            PolicyVerdict::RequireApproval
        } else {
            PolicyVerdict::Allow
        }
    }
}

fn cmp_str(op: Cmp) -> &'static str {
    match op {
        Cmp::Eq => "==",
        Cmp::Ne => "!=",
        Cmp::Lt => "<",
        Cmp::Le => "<=",
        Cmp::Gt => ">",
        Cmp::Ge => ">=",
    }
}

/// 数值感知比较（Eq/Ne 全等；其余按 f64）。
fn compare(a: &Value, b: &Value, op: Cmp) -> bool {
    match op {
        Cmp::Eq => a == b,
        Cmp::Ne => a != b,
        _ => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => match op {
                Cmp::Lt => x < y,
                Cmp::Le => x <= y,
                Cmp::Gt => x > y,
                Cmp::Ge => x >= y,
                _ => unreachable!(),
            },
            _ => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn precondition_and_approval_gates() {
        let p = Policy {
            require_approval: true,
            preconditions: vec![
                Precondition {
                    param: "amount".into(),
                    op: Cmp::Le,
                    value: json!(1000),
                },
                Precondition {
                    param: "level".into(),
                    op: Cmp::Ne,
                    value: json!("BLACKLIST"),
                },
            ],
        };
        // 全部满足 + 需审批 → RequireApproval
        assert_eq!(
            p.check(&json!({ "amount": 500, "level": "VIP" })),
            PolicyVerdict::RequireApproval
        );
        // 前置不满足 → Deny（优先于审批）
        assert!(matches!(
            p.check(&json!({ "amount": 5000, "level": "VIP" })),
            PolicyVerdict::Deny(_)
        ));
        assert!(matches!(
            p.check(&json!({ "amount": 500, "level": "BLACKLIST" })),
            PolicyVerdict::Deny(_)
        ));
        // 无审批 → Allow
        let p2 = Policy::default();
        assert_eq!(p2.check(&json!({})), PolicyVerdict::Allow);
    }
}
