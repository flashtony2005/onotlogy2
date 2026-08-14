#!/usr/bin/env python3
"""planner.rs → planner/ 目录切分（纯机械搬移 + 最小可见性调整，行为零变化）。"""
import os, shutil

SRC = "src/planner.rs"
OUT = "src/planner"
lines = open(SRC).read().split("\n")

# 顶层符号（1-based 行号），取自 grep 输出
symbols = [
    ("APPROVAL_TIMEOUT", 28), ("lock_mutex", 32), ("RuntimeConfig", 39),
    ("ApprovalAuditor", 47), ("may_auto_approve", 53), ("PlannerError", 97),
    ("RunStatus", 113), ("AgentRunResult", 144), ("Capability", 154),
    ("PlanStep", 187), ("Policy", 195), ("PolicyApprover", 203),
    ("AutoApprover", 210), ("ConsoleApprover", 224), ("prompt", 269),
    ("RunAuditor", 284), ("ApprovalState", 315), ("ApprovalRequest", 348),
    ("ApprovalStore", 359), ("ApprovalEntry", 363), ("ServerApprover", 479),
    ("Policy_impl", 604), ("resolve_policy_field", 682), ("resolve_value", 698),
    ("step_key", 718), ("action_matches_intent", 731), ("intent_action_allowlist", 738),
    ("classify_intent_text", 742), ("contains_any", 765), ("extract_action_args", 771),
    ("extract_named_param", 805), ("strip_object_ids", 829), ("token_in_text", 836),
    ("resolve_target_ids", 842), ("world_object_ids", 867), ("object_in_scope", 875),
    ("scan_numbers", 893), ("number_as", 916), ("extract_bool", 932),
    ("action_preconditions_hold", 947), ("compare_values", 963), ("PolicyDecision", 983),
    ("LLMGoalParser", 989), ("Goal", 1021), ("Planner", 1028),
    ("call_engine", 1650), ("non_empty", 1678), ("now_micros", 1687),
    ("new_run_id", 1696), ("generate_run_id", 1706), ("intent_filter_tests", 1710),
]
names = [n for n, _ in symbols]
starts = {n: l - 1 for n, l in symbols}
def attr_prefix(idx):
    """从 idx-1 向上收集紧邻的属性行（#[derive] / #[async_trait] / 文档注释）。"""
    i = idx - 1
    out = []
    while i >= 0:
        t = lines[i].strip()
        if t.startswith("#[") or t.startswith("///") or t.startswith("//!"):
            out.append(i)
            i -= 1
        else:
            break
    return out
los = {}
for name in names:
    lo = starts[name]
    prefix = attr_prefix(lo)
    los[name] = min(prefix) if prefix else lo
ends = {names[i]: los[names[i + 1]] for i in range(len(names) - 1)}
ends[names[-1]] = len(lines)

FILES = {
    "mod.rs": ["RuntimeConfig", "PlannerError", "RunStatus", "AgentRunResult", "Capability", "PlanStep", "Policy", "PolicyDecision"],
    "util.rs": ["APPROVAL_TIMEOUT", "lock_mutex", "non_empty", "now_micros", "new_run_id", "generate_run_id"],
    "approval.rs": ["ApprovalAuditor", "PolicyApprover", "AutoApprover", "ConsoleApprover", "prompt", "RunAuditor", "ApprovalState", "ApprovalRequest", "ApprovalStore", "ApprovalEntry", "ServerApprover"],
    "policy.rs": ["may_auto_approve", "Policy_impl", "resolve_policy_field", "resolve_value", "step_key", "compare_values"],
    "intent.rs": ["action_matches_intent", "intent_action_allowlist", "classify_intent_text", "contains_any", "extract_action_args", "extract_named_param", "strip_object_ids", "token_in_text", "resolve_target_ids", "world_object_ids", "object_in_scope", "scan_numbers", "number_as", "extract_bool", "action_preconditions_hold", "intent_filter_tests"],
    "run.rs": ["LLMGoalParser", "Goal", "Planner", "call_engine"],
}

def extract(name):
    block = lines[los[name]:ends[name]]
    while block and block[-1].strip() == "":
        block.pop()
    return "\n".join(block)

DOC = """//! Planner 编排闭环（对应 REQ-011 / prd-agent-integration.md 4.3）
//!
//! OpenAIP 自有核心：**Goal → LLMGoalParser → Planner → PlanStep → Capability → Execute**。
//! Pi 仅作 Cognitive Engine（经 `AgentEngine::reason` 提供 NLU/推理），本模块拥有编排与决策。
//! 每步产出 cognitive `TraceEvent` 并绑定 `object_refs`（可解释性根基）；Policy 拒 → WB2005；
//! 认知引擎不可用 → WB2004。
//!
//! 模块拆分（长文件模块化）：核心类型在 `mod`，审批闸门在 `approval`，策略校验在 `policy`，
//! 意图过滤/参数抽取在 `intent`，主流程在 `run`，工具函数在 `util`。

"""

MOD_PUB = """pub mod approval;
pub mod intent;
pub mod policy;
pub mod run;
pub mod util;

pub use approval::{
    ApprovalAuditor, ApprovalRequest, ApprovalState, ApprovalStore, AutoApprover,
    ConsoleApprover, PolicyApprover, RunAuditor, ServerApprover,
};
pub use run::Planner;
pub use util::{APPROVAL_TIMEOUT, generate_run_id, lock_mutex};

"""

# 各文件 use 前缀
PREFIX = {
    "mod.rs": """use crate::action::ActionError;
use crate::storage::StorageError;
use serde_json::Value;

""",
    "util.rs": """use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

""",
    "approval.rs": """use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;
use tokio::sync::oneshot;

use crate::runtime::RuntimeKernel;
use async_trait::async_trait;
use serde_json::json;

use super::util::lock_mutex;
use super::util::now_micros;
use super::*;

""",
    "policy.rs": """use std::collections::HashMap;

use crate::evolution::LearnedKnowledge;
use crate::ir::{ActionDef, CompareOp, PolicyDef, ScalarType, ValueExpr};
use serde_json::Value;

use super::approval::ApprovalAuditor;
use super::approval::PolicyApprover;
use super::*;

""",
    "intent.rs": """use std::collections::HashMap;

use crate::agent::WorldSnapshot;
use crate::ir::{ActionDef, ParamDef, ScalarType};
use serde_json::Value;

use super::policy::compare_values;
use super::policy::resolve_policy_field;
use super::policy::resolve_value;
use super::*;

""",
    "run.rs": """use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::action::{ActionEngine, ActionError, ActionRegistry};
use crate::agent::{AgentEngine, AgentError, Context, Decision, WorldSnapshot};
use crate::runtime::RuntimeKernel;
use crate::storage::StorageError;
use crate::tool_backend::{ActionHook, NullBackend, ToolBackend, ToolOutcome};
use async_trait::async_trait;
use serde_json::{Value, json};

use super::approval::{ApprovalAuditor, PolicyApprover, RunAuditor};
use super::intent::action_matches_intent;
use super::intent::action_preconditions_hold;
use super::intent::extract_action_args;
use super::intent::object_in_scope;
use super::intent::resolve_target_ids;
use super::intent::world_object_ids;
use super::policy::{Policy, PolicyDecision, step_key};
use super::util::generate_run_id;
use super::*;

""",
}

# 可见性调整：兄弟模块跨文件引用 → pub(super)
VIS = {
    "policy.rs": [
        ("fn resolve_policy_field", "pub(super) fn resolve_policy_field"),
        ("fn resolve_value", "pub(super) fn resolve_value"),
        ("fn step_key", "pub(super) fn step_key"),
        ("fn compare_values", "pub(super) fn compare_values"),
    ],
    "intent.rs": [
        ("fn action_matches_intent", "pub(super) fn action_matches_intent"),
        ("fn extract_action_args", "pub(super) fn extract_action_args"),
        ("fn action_preconditions_hold", "pub(super) fn action_preconditions_hold"),
        ("fn object_in_scope", "pub(super) fn object_in_scope"),
        ("fn resolve_target_ids", "pub(super) fn resolve_target_ids"),
        ("fn world_object_ids", "pub(super) fn world_object_ids"),
    ],
    "util.rs": [
        ("fn now_micros", "pub(super) fn now_micros"),
        ("fn lock_mutex", "pub(crate) fn lock_mutex"),  # 已是 pub(crate)，幂等
    ],
}

os.makedirs(OUT, exist_ok=True)
with open(f"{OUT}/mod.rs", "w") as f:
    f.write(DOC + MOD_PUB + PREFIX["mod.rs"] + "\n" + "\n\n".join(extract(n) for n in FILES["mod.rs"]) + "\n")
for fname in ["util.rs", "approval.rs", "policy.rs", "intent.rs", "run.rs"]:
    body = "\n\n".join(extract(n) for n in FILES[fname])
    for old, new in VIS.get(fname, []):
        assert old in body, f"{fname}: {old} not found"
        body = body.replace(old, new, 1)
    with open(f"{OUT}/{fname}", "w") as f:
        f.write(PREFIX[fname] + body + "\n")
shutil.move(SRC, SRC + ".bak")
print("split done:", os.listdir(OUT))
