use std::collections::HashMap;

use crate::agent::WorldSnapshot;
use crate::ir::{ActionDef, ScalarType};
use serde_json::Value;

use super::policy::compare_values;
use super::policy::resolve_policy_field;
use super::policy::resolve_value;

/// 用户目标优先，其次解析出的 Intent；都认不出则不过滤（只靠前置条件）。
pub(super) fn action_matches_intent(action: &str, goal: &str, intent: &str) -> bool {
    match intent_action_allowlist(goal, intent) {
        None => true,
        Some(allow) => allow.iter().any(|a| a.eq_ignore_ascii_case(action)),
    }
}

fn intent_action_allowlist(goal: &str, intent: &str) -> Option<&'static [&'static str]> {
    classify_intent_text(goal).or_else(|| classify_intent_text(intent))
}

fn classify_intent_text(text: &str) -> Option<&'static [&'static str]> {
    let t = text.to_ascii_lowercase();
    if contains_any(&t, &["lint", "clippy", "质量", "静态检查"]) {
        return Some(&["lint"]);
    }
    if contains_any(&t, &["refund", "退款"]) {
        return Some(&["refund"]);
    }
    if contains_any(
        &t,
        &["close account", "closeaccount", "关闭账号", "关闭账户"],
    ) {
        return Some(&["closeaccount"]);
    }
    if contains_any(
        &t,
        &["retain", "retention", "churn", "挽留", "task", "任务"],
    ) {
        return Some(&["retain", "start", "complete"]);
    }
    None
}

fn contains_any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

/// 从目标/Intent 抽取动作参数：只填 ODL `params:` 声明过的字段。
/// 具名 `amount=500` 优先；否则 Int/Float 取文本中最后一个数字（避免把对象 id 当成金额）。
pub(super) fn extract_action_args(
    act: &ActionDef,
    goal: &str,
    intent: &str,
    world_ids: &[String],
) -> HashMap<String, Value> {
    let hay = format!("{goal} {intent}");
    let for_nums = strip_object_ids(&hay, world_ids);
    let nums = scan_numbers(&for_nums);
    let mut out = HashMap::new();
    for p in &act.params {
        if let Some(v) = extract_named_param(&hay, &p.name, p.ty) {
            out.insert(p.name.clone(), v);
            continue;
        }
        match p.ty {
            ScalarType::Int | ScalarType::Float => {
                if let Some(&n) = nums.last()
                    && let Some(v) = number_as(p.ty, n)
                {
                    out.insert(p.name.clone(), v);
                }
            }
            ScalarType::Bool => {
                if let Some(b) = extract_bool(&hay) {
                    out.insert(p.name.clone(), Value::Bool(b));
                }
            }
            ScalarType::String => {}
        }
    }
    out
}

fn extract_named_param(hay: &str, name: &str, ty: ScalarType) -> Option<Value> {
    let lower = hay.to_ascii_lowercase();
    let key = name.to_ascii_lowercase();
    let idx = lower.find(&key)?;
    let rest = hay[idx + name.len()..].trim_start();
    let rest = rest
        .strip_prefix(['=', ':', '：'])
        .map(str::trim_start)
        .unwrap_or(rest);
    match ty {
        ScalarType::Int | ScalarType::Float => {
            let n = scan_numbers(rest).into_iter().next()?;
            number_as(ty, n)
        }
        ScalarType::Bool => extract_bool(rest).map(Value::Bool),
        ScalarType::String => {
            let tok = rest.split_whitespace().next()?;
            Some(Value::String(
                tok.trim_matches(|c| c == '"' || c == '\'').to_string(),
            ))
        }
    }
}

fn strip_object_ids(hay: &str, ids: &[String]) -> String {
    hay.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.' && c != '-')
        .filter(|t| !t.is_empty() && !ids.iter().any(|id| id == t))
        .collect::<Vec<_>>()
        .join(" ")
}

fn token_in_text(text: &str, id: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.' && c != '-')
        .any(|t| t == id)
}

/// 目标文本点名优先；否则用解析 refs（去掉 world 哨兵）；都没有则不限制（None）。
pub(super) fn resolve_target_ids(
    goal: &str,
    parsed_refs: &[String],
    world_ids: &[String],
) -> Option<Vec<String>> {
    let mentioned: Vec<String> = world_ids
        .iter()
        .filter(|id| token_in_text(goal, id))
        .cloned()
        .collect();
    if !mentioned.is_empty() {
        return Some(mentioned);
    }
    let parsed: Vec<String> = parsed_refs
        .iter()
        .filter(|r| !r.is_empty() && r.as_str() != "world" && world_ids.iter().any(|id| id == *r))
        .cloned()
        .collect();
    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
}

pub(super) fn world_object_ids(world: &WorldSnapshot) -> Vec<String> {
    world
        .objects
        .iter()
        .filter_map(|o| o.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect()
}

pub(super) fn object_in_scope(obj: &Value, scope: &[String]) -> bool {
    let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("");
    if scope.iter().any(|s| s == id) {
        return true;
    }
    if scope.iter().any(|s| id.starts_with(&format!("{s}_"))) {
        return true;
    }
    if let Some(cust) = obj
        .get("data")
        .and_then(|d| d.get("customer"))
        .and_then(|v| v.as_str())
    {
        return scope.iter().any(|s| s == cust);
    }
    false
}

fn scan_numbers(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() || ch == '.' || (ch == '-' && cur.is_empty()) {
            cur.push(ch);
        } else if !cur.is_empty() {
            if let Ok(n) = cur.parse::<f64>() {
                out.push(n);
            }
            cur.clear();
        }
    }
    if !cur.is_empty()
        && let Ok(n) = cur.parse::<f64>()
    {
        out.push(n);
    }
    out
}

/// 数值参数转换：Int 要求整数值且落在 i64 值域内，否则返回 None（不填该参数）。
/// 避免目标文本里的 `1e100` 之类被饱和转换成 `i64::MAX` 后静默进入动作参数。
fn number_as(ty: ScalarType, n: f64) -> Option<Value> {
    if !n.is_finite() {
        return None;
    }
    match ty {
        ScalarType::Int => {
            if n.fract() != 0.0 || n < i64::MIN as f64 || n > i64::MAX as f64 {
                return None;
            }
            Some(Value::from(n as i64))
        }
        _ => Some(Value::from(n)),
    }
}

/// 词边界匹配布尔词（避免 `contains("no")` 误中 know/now 等词）；中文「是/否」按词匹配。
fn extract_bool(s: &str) -> Option<bool> {
    let t = s.to_ascii_lowercase();
    let words = t
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>();
    if words.iter().any(|w| matches!(*w, "true" | "yes" | "是")) {
        Some(true)
    } else if words.iter().any(|w| matches!(*w, "false" | "no" | "否")) {
        Some(false)
    } else {
        None
    }
}

pub(super) fn action_preconditions_hold(
    act: &ActionDef,
    target: Option<&Value>,
    args: &HashMap<String, Value>,
) -> bool {
    for clause in &act.policy.precondition {
        let lhs = resolve_policy_field(&clause.field, args, target);
        let rhs = resolve_value(&clause.value, args, target, "");
        if !compare_values(&lhs, &rhs, clause.op) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod intent_filter_tests {
    use super::*;

    #[test]
    fn goal_text_wins_over_generic_intent() {
        assert!(action_matches_intent(
            "closeAccount",
            "close account",
            "Customer Retention"
        ));
        assert!(!action_matches_intent(
            "retain",
            "close account",
            "Customer Retention"
        ));
    }

    #[test]
    fn retention_goal_excludes_refund() {
        assert!(action_matches_intent(
            "retain",
            "manage retention tasks",
            ""
        ));
        assert!(action_matches_intent("start", "manage retention tasks", ""));
        assert!(action_matches_intent(
            "complete",
            "manage retention tasks",
            ""
        ));
        assert!(!action_matches_intent(
            "refund",
            "manage retention tasks",
            ""
        ));
    }

    #[test]
    fn refund_goal_only_allows_refund() {
        assert!(action_matches_intent(
            "refund",
            "退款",
            "Customer Retention"
        ));
        assert!(!action_matches_intent(
            "retain",
            "退款",
            "Customer Retention"
        ));
    }

    #[test]
    fn unknown_goal_does_not_filter() {
        assert!(action_matches_intent("refund", "hello world", "unrelated"));
        assert!(action_matches_intent("retain", "hello world", "unrelated"));
    }

    fn amount_param() -> ActionDef {
        ActionDef {
            name: "refund".into(),
            params: vec![crate::ir::ParamDef {
                name: "amount".into(),
                ty: ScalarType::Float,
            }],
            updates: vec![],
            creates: vec![],
            policy: crate::ir::PolicyDef::default(),
        }
    }

    #[test]
    fn extract_last_number_as_amount() {
        let args = extract_action_args(&amount_param(), "refund this customer 500", "", &[]);
        assert_eq!(args.get("amount").and_then(|v| v.as_f64()), Some(500.0));
    }

    #[test]
    fn extract_named_amount_wins() {
        let args = extract_action_args(
            &amount_param(),
            "refund customer 001 amount=80",
            "",
            &["001".into()],
        );
        assert_eq!(args.get("amount").and_then(|v| v.as_f64()), Some(80.0));
    }

    #[test]
    fn extract_skips_when_no_number() {
        let args = extract_action_args(&amount_param(), "manage retention tasks", "", &[]);
        assert!(args.is_empty());
    }

    #[test]
    fn extract_skips_object_id_as_amount() {
        let args = extract_action_args(&amount_param(), "refund 001 500", "", &["001".into()]);
        assert_eq!(args.get("amount").and_then(|v| v.as_f64()), Some(500.0));
    }

    #[test]
    fn extract_rejects_int_out_of_range() {
        // Int 参数遇超 i64 值域的数（999...999 24 位 → f64 1e24）不得饱和成 i64::MAX 静默进入动作参数
        let act = ActionDef {
            name: "quota".into(),
            params: vec![crate::ir::ParamDef {
                name: "quota".into(),
                ty: ScalarType::Int,
            }],
            updates: vec![],
            creates: vec![],
            policy: crate::ir::PolicyDef::default(),
        };
        let args = extract_action_args(&act, "set quota 999999999999999999999999", "", &[]);
        assert!(
            !args.contains_key("quota"),
            "越界数值应被拒绝而非饱和转换，实际: {args:?}"
        );
        // 值域内整数正常提取
        let ok = extract_action_args(&act, "set quota 42", "", &[]);
        assert_eq!(ok.get("quota").and_then(|v| v.as_i64()), Some(42));
    }

    #[test]
    fn resolve_prefers_goal_mentioned_ids() {
        let world = vec!["001".into(), "002".into()];
        let got = resolve_target_ids("refund 001 500", &["002".into()], &world);
        assert_eq!(got, Some(vec!["001".into()]));
    }

    #[test]
    fn resolve_falls_back_to_parsed_refs() {
        let world = vec!["001".into(), "002".into()];
        let got = resolve_target_ids(
            "manage retention tasks",
            &["001".into(), "world".into()],
            &world,
        );
        assert_eq!(got, Some(vec!["001".into()]));
    }

    #[test]
    fn resolve_unrestricted_when_nothing_named() {
        let world = vec!["001".into()];
        assert_eq!(resolve_target_ids("hello", &["world".into()], &world), None);
    }
}
