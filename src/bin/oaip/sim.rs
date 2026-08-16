//! 模拟命令（M5 拆分）：S1 回放 / S2·S3 投影（假设轨）。

use openaip_os::storage::LibSQLStorage;

pub async fn cmd_replay(db: &str) -> Result<(), Box<dyn std::error::Error>> {
    let storage = LibSQLStorage::open(db).await?;
    let report = openaip_os::simulation::replay::run_decision_replay(&storage).await?;
    println!(
        "🔁 Decision Replay（Learning Counterfactual）：{} 条决策，{} 条会因当前经验改变（{:.0}%）",
        report.total_decisions,
        report.changed,
        report.change_rate * 100.0
    );
    for d in &report.decisions {
        if d.changed {
            println!(
                "   ⚡ run={} capability={} actual={} → replay={}",
                d.run_id, d.capability, d.actual_worker, d.replay_worker
            );
        }
    }
    if report.decisions.is_empty() {
        println!("   （无历史决策数据——先跑几次 oaip worker run 积累经验）");
    }
    Ok(())
}

/// Simulation（S2/S3）：当前世界状态 + 假设动作 → 投影对比 → 决策支撑。
/// 铁律：产出是 Projection（假设轨），永不写入 EventLog（I2）。
pub async fn cmd_simulate(
    db: &str,
    a_json: &str,
    b_json: &str,
    worker_a: Option<&str>,
    worker_b: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    use openaip_os::simulation::projection::HypotheticalAction;

    let storage = LibSQLStorage::open(db).await?;
    let a: HypotheticalAction = serde_json::from_str(a_json)?;
    let mut b: HypotheticalAction = serde_json::from_str(b_json)?;
    if let Some(w) = worker_a {
        // worker_id 是动作的一部分（S3 worker 维度）；从参数注入
        let mut a = a;
        a.worker_id = Some(w.to_string());
        return simulate_inner(&storage, &a, &mut b, worker_b).await;
    }
    let _ = &mut b;
    simulate_inner(&storage, &a, &mut b, worker_b).await
}

async fn simulate_inner(
    storage: &LibSQLStorage,
    a: &openaip_os::simulation::projection::HypotheticalAction,
    b: &mut openaip_os::simulation::projection::HypotheticalAction,
    worker_b: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    use openaip_os::simulation::projection::{counterfactual, project};
    use openaip_os::worker::policy::WorkerPolicy;

    if let Some(w) = worker_b {
        b.worker_id = Some(w.to_string());
    }

    // 状态：真实世界关系集（Decision Support 基于当前世界）
    let rels = storage.list_relations().await?;
    let state: Vec<(String, String, String)> = rels
        .iter()
        .map(|r| (r.source.clone(), r.relation.clone(), r.target.clone()))
        .collect();
    println!("🌍 当前世界状态：{} 条关系", state.len());
    for (s, p, t) in &state {
        println!("   {s} -[{p}]-> {t}");
    }

    // Policy（与 worker run 一致：shell.execute/code.write 需审批，其余 allow）
    let mut policy = WorkerPolicy::new();
    for cap in [&a.capability, &b.capability].into_iter().flatten() {
        policy = match cap.as_str() {
            "shell.execute" | "code.write" => policy.require_approval(cap),
            _ => policy.allow(cap),
        };
    }

    // Worker 信任维度（诚实边界）：动作级模拟用 declared trust——learned 是 (capability, worker)
    // 级修正，作用域是路由决策（S1 replay 已覆盖）；此处不跨作用域借用。
    let manifests = storage.list_workers().await?;
    let mut worker_scores: Vec<(String, f64)> =
        manifests.iter().map(|m| (m.id.clone(), m.trust)).collect();
    worker_scores.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));
    if let Some(w) = &b.worker_id
        && !worker_scores.iter().any(|(id, _)| id == w)
    {
        eprintln!(
            "⚠️ 未知 Worker id「{w}」——可用：{}",
            worker_scores
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>()
                .join(" / ")
        );
    }

    // S2：两投影
    let pa = project(&state, a);
    let pb = project(&state, b);
    println!("🔮 S2 投影（假设轨，永不写入事件源）");
    println!(
        "   A「{}」→ 新增 {} 条关系（可逆性 {}）",
        a.action,
        pa.predicted_state["added"]
            .as_array()
            .map(|x| x.len())
            .unwrap_or(0),
        a.reversibility
    );
    println!(
        "   B「{}」→ 新增 {} 条关系（可逆性 {}）",
        b.action,
        pb.predicted_state["added"]
            .as_array()
            .map(|x| x.len())
            .unwrap_or(0),
        b.reversibility
    );

    // S3：反事实对比
    let report = counterfactual(&state, a, b, &policy, &worker_scores);
    println!("⚖️ S3 反事实对比");
    for d in &report.dimensions {
        println!(
            "   「{}」outcome={} risk={} policy={} worker_trust={:?} → {}",
            d.action,
            d.outcome_added,
            d.risk_level,
            d.policy,
            d.worker_trust,
            if d.recommended {
                "✓ 可执行"
            } else {
                "✗ 不可自动执行"
            }
        );
    }
    match &report.recommendation {
        Some(r) => println!("🎯 推荐：{r}（确定性规则：Policy 允许 → 低风险 → worker 信任）"),
        None => println!("🎯 无推荐（双方均不可自动执行——Simulation 不能绕过 Governance）"),
    }
    Ok(())
}
