//! V0.4 Policy 引擎测试契约（TC-015a~d + TC-021a/b）
mod common;
use common::*;

use openaip_os::action::ActionRegistry;
use openaip_os::ir::{CompareOp, ValueExpr};
use openaip_os::parse_odl;
use openaip_os::planner::{
    AutoApprover, Capability, PlanStep, Planner, PlannerError, Policy, PolicyDecision,
    RuntimeConfig, generate_run_id,
};
use openaip_os::runtime::RuntimeKernel;
use openaip_os::storage::LibSQLStorage;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

// ── TC-015a：ODL `policy:` 块解析为 IR（require_approval + precondition 子句） ──────────
#[test]
fn tc015a_odl_parses_policy() {
    let ir = parse_odl(ODL_REFUND).expect("ODL 应解析");
    let cust = ir.find("Customer").expect("应有 Customer");
    let refund = cust
        .actions
        .iter()
        .find(|a| a.name == "refund")
        .expect("refund");
    assert!(refund.policy.require_approval, "refund 应需审批");

    assert_eq!(refund.policy.precondition.len(), 2, "应有 2 条前置条件");
    let c0 = &refund.policy.precondition[0];
    assert_eq!(c0.field, "amount");
    assert_eq!(c0.op, CompareOp::Le);
    assert_eq!(
        c0.value,
        ValueExpr::Literal(json!(1000)),
        "比较右值应为字面量 1000"
    );
    let c1 = &refund.policy.precondition[1];
    assert_eq!(c1.field, "self.level");
    assert_eq!(c1.op, CompareOp::Ne);
    assert!(matches!(c1.value, ValueExpr::Literal(Value::String(ref s)) if s == "BLACKLIST"));
}

// ── TC-015b：审批闸门 — require_approval + AutoApprover 放行/拒绝 ───────────────────────
#[tokio::test]
async fn tc015b_approval_gate() {
    let def = parse_odl(ODL_REFUND)
        .unwrap()
        .find("Customer")
        .unwrap()
        .actions
        .iter()
        .find(|a| a.name == "refund")
        .cloned()
        .unwrap();
    let step = PlanStep {
        capability: Capability::Action {
            object_type: "Customer".into(),
            action: "refund".into(),
            target_id: "001".into(),
            args: json!({}),
        },
        object_refs: vec!["001".into()],
        description: "refund".into(),
    };

    // 默认审批器（allow=false）→ 需 Human Approval，Deny（WB2005）
    let mut args = HashMap::new();
    args.insert("amount".to_string(), json!(500));
    let target = json!({ "level": "VIP" });
    let deny = Policy::check_action(
        &step,
        Some(&def),
        &AutoApprover { allow: false },
        Some(&target),
        &args,
        &RuntimeConfig::default(),
        None,
        0,
        None,
    )
    .await;
    assert!(matches!(deny, PolicyDecision::Deny { .. }), "未批准应 Deny");

    // 自动批准（allow=true）→ Allow
    let allow = Policy::check_action(
        &step,
        Some(&def),
        &AutoApprover { allow: true },
        Some(&target),
        &args,
        &RuntimeConfig::default(),
        None,
        0,
        None,
    )
    .await;
    assert_eq!(allow, PolicyDecision::Allow, "批准后应 Allow");
}

// ── TC-015c：前置条件 — 参数/self 字段比较满足或拒绝 ───────────────────────────────────
#[tokio::test]
async fn tc015c_precondition_eval() {
    let def = parse_odl(ODL_REFUND)
        .unwrap()
        .find("Customer")
        .unwrap()
        .actions
        .iter()
        .find(|a| a.name == "refund")
        .cloned()
        .unwrap();
    let step = PlanStep {
        capability: Capability::Action {
            object_type: "Customer".into(),
            action: "refund".into(),
            target_id: "001".into(),
            args: json!({}),
        },
        object_refs: vec!["001".into()],
        description: "refund".into(),
    };
    let approver = AutoApprover { allow: true };

    // amount=500 <= 1000 且 level != BLACKLIST → Allow
    let mut args = HashMap::new();
    args.insert("amount".to_string(), json!(500));
    let target = json!({ "id": "001", "level": "VIP" });
    assert_eq!(
        Policy::check_action(
            &step,
            Some(&def),
            &approver,
            Some(&target),
            &args,
            &RuntimeConfig::default(),
            None,
            0,
            None
        )
        .await,
        PolicyDecision::Allow,
        "金额 500 且非黑名单应 Allow"
    );

    // amount=2000 > 1000 → Deny（不满足 precondition）
    let mut args2 = HashMap::new();
    args2.insert("amount".to_string(), json!(2000));
    assert!(
        matches!(
            Policy::check_action(
                &step,
                Some(&def),
                &approver,
                Some(&target),
                &args2,
                &RuntimeConfig::default(),
                None,
                0,
                None
            )
            .await,
            PolicyDecision::Deny { .. }
        ),
        "金额 2000 超阈值应 Deny"
    );

    // level=BLACKLIST → Deny（self.level 不满足 ne）
    let blacklisted = json!({ "id": "001", "level": "BLACKLIST" });
    let mut args3 = HashMap::new();
    args3.insert("amount".to_string(), json!(500));
    assert!(
        matches!(
            Policy::check_action(
                &step,
                Some(&def),
                &approver,
                Some(&blacklisted),
                &args3,
                &RuntimeConfig::default(),
                None,
                0,
                None
            )
            .await,
            PolicyDecision::Deny { .. }
        ),
        "黑名单客户应 Deny"
    );
}

// ── TC-015d：Planner 闭环 — require_approval 动作默认拒（WB2005）；自动批准则跑通 ───────
#[tokio::test]
async fn tc015d_planner_policy_gate() {
    let s = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel = RuntimeKernel::new(s);
    kernel
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    let reg = ActionRegistry::from_ir(&parse_odl(ODL_APPROVE).unwrap());

    // 默认（无自动批准）→ 应因 Policy 拒绝而返回 WB2005
    let run_id = generate_run_id();
    let res = Planner::with_actions(&kernel, reg.clone())
        .run_goal(&FakeEngine, "close account", "retention-agent", &run_id)
        .await;
    match res {
        Err(PlannerError::PolicyDenied(msg)) => {
            assert!(msg.contains("Human Approval"), "应提示需人工审批: {msg}")
        }
        other => panic!("require_approval 动作应被 Policy 拒绝（WB2005），得到 {other:?}"),
    }

    // 注入自动批准 → 闭环跑通（Completed），动作被实际执行
    let s2 = LibSQLStorage::open(":memory:").await.unwrap();
    let kernel2 = RuntimeKernel::new(s2);
    kernel2
        .create_object("Customer", "001", json!({"name": "Apple", "level": "VIP"}))
        .await
        .unwrap();
    let run_id2 = generate_run_id();
    let res2 = Planner::with_actions(&kernel2, reg)
        .with_approver(Arc::new(AutoApprover { allow: true }))
        .run_goal(&FakeEngine, "close account", "retention-agent", &run_id2)
        .await
        .expect("自动批准后应跑通");
    assert_eq!(res2.status.as_str(), "Completed");
    // 动作在自动批准后被执行：产出 action.closeAccount world 事件
    let wevents = kernel2.list_world_events().await.unwrap();
    assert!(
        wevents
            .iter()
            .any(|e| e.event_type == "action.closeAccount"),
        "自动批准后动作应被执行"
    );
}

// ── TC-021a：CLI `agent run` 与 `serve` 暴露 `--auto-approve` 开关（防回归） ─────────────
#[test]
fn tc021a_cli_auto_approve_flag_exists() {
    use std::process::Command;
    let bin = env!("CARGO_BIN_EXE_oaip");
    // agent run --help 应含 --auto-approve
    let out = Command::new(bin)
        .args(["agent", "run", "--help"])
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(
        help.contains("--auto-approve"),
        "agent run 应暴露 --auto-approve 开关"
    );
    // serve --help 应含 --auto-approve
    let out2 = Command::new(bin)
        .args(["serve", "--help"])
        .output()
        .unwrap();
    let help2 = String::from_utf8_lossy(&out2.stdout);
    assert!(
        help2.contains("--auto-approve"),
        "serve 应暴露 --auto-approve 开关"
    );
}

// ── TC-021b：ConsoleApprover 公开导出且可构造（交互式 Human Approval 入口） ───────────
#[test]
fn tc021b_console_approver_constructible() {
    use openaip_os::planner::ConsoleApprover;
    let _ = ConsoleApprover::new();
}

// 注：ConsoleApprover 的 stdin 交互（y/N）属 CLI 运行时行为，需人工验证：
//   `oaip agent run --odl examples/crm.odl "reduce churn"`（含 require_approval 动作时
//   每动作交互询问一次，y 放行 / N 拒=WB2005 终止）；`--auto-approve` 直接放行。
