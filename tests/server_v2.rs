//! V2 认知面板 API（Phase 10 · UI 后端）集成测试。
//!
//! SV1 空列表 → Import Skill → 列表可见
//! SV2 Plugin 列表（生命周期状态）
//! SV3 Session 创建 + 事件读取（append 后可查）
//! SV4 未登录访问被拒（auth 中间件生效）——独立于 enforce_auth=false 的基础路径

mod common;

use openaip_os::runtime::RuntimeKernel;
use serde_json::{Value, json};

async fn call(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let url = format!("http://{addr}{path}");
    let client = reqwest::Client::new();
    let mut req = client.request(
        reqwest::Method::from_bytes(method.as_bytes()).unwrap(),
        &url,
    );
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await.unwrap();
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(json!(null));
    (status, body)
}

#[tokio::test]
async fn tc_sv1_import_skill_then_list() {
    let kernel = std::sync::Arc::new(RuntimeKernel::new(
        openaip_os::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap(),
    ));
    let addr = common::spawn_server(kernel).await;

    // 空列表
    let (s0, body0) = call(addr, "GET", "/api/v1/v2/skills", None).await;
    assert_eq!(s0, 200);
    assert_eq!(body0.as_array().unwrap().len(), 0, "初始为空");

    // Import
    let md = "# Thumbnail Generator\n\n- goal: 生成缩略图\n## Stage 1: Design\n- capability: image.generate\n- artifact: ThumbnailConcept\n## Stage 2: Render\n- capability: image.render\n- artifact: ThumbnailFile\n";
    let (s1, body1) = call(
        addr,
        "POST",
        "/api/v1/v2/skills/import",
        Some(json!({ "name": "thumbnail-gen", "content": md, "known_capabilities": ["image.generate", "image.render"] })),
    )
    .await;
    assert_eq!(s1, 200, "导入成功: {body1}");
    assert_eq!(body1["id"], "thumbnail-gen");
    assert_eq!(body1["skill_type"], "Workflow");

    // 列表可见
    let (s2, body2) = call(addr, "GET", "/api/v1/v2/skills", None).await;
    assert_eq!(s2, 200);
    let arr = body2.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["capabilities"][0], "image.generate");

    // 重复导入 → 409
    let (s3, _) = call(
        addr,
        "POST",
        "/api/v1/v2/skills/import",
        Some(json!({ "name": "thumbnail-gen", "content": md, "known_capabilities": ["image.generate", "image.render"] })),
    )
    .await;
    assert_eq!(s3, 409, "重复注册拒绝");
}

#[tokio::test]
async fn tc_sv1b_skill_raw_view() {
    let kernel = std::sync::Arc::new(RuntimeKernel::new(
        openaip_os::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap(),
    ));
    let addr = common::spawn_server(kernel).await;

    let md = "# Demo Skill\n\n- goal: 演示\n## Stage 1: A\n- capability: x.y\n## Stage 2: B\n- capability: x.z\n";
    let (s0, _) = call(
        addr,
        "POST",
        "/api/v1/v2/skills/import",
        Some(json!({ "name": "demo-skill", "content": md, "known_capabilities": ["x.y", "x.z"] })),
    )
    .await;
    assert_eq!(s0, 200);

    // 原始内容可查看（Raw 不修改）
    let (s1, body1) = call(addr, "GET", "/api/v1/v2/skills/demo-skill/raw", None).await;
    assert_eq!(s1, 200, "raw 端点: {body1}");
    assert_eq!(body1["content"], md, "原始内容逐字保留");
    assert!(
        !body1["content_hash"].as_str().unwrap().is_empty(),
        "hash 可追溯"
    );

    // 不存在 → 404
    let (s2, _) = call(addr, "GET", "/api/v1/v2/skills/nope/raw", None).await;
    assert_eq!(s2, 404);
}

#[tokio::test]
async fn tc_sv2_plugins_list() {
    let kernel = std::sync::Arc::new(RuntimeKernel::new(
        openaip_os::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap(),
    ));
    let addr = common::spawn_server(kernel).await;
    let (s, body) = call(addr, "GET", "/api/v1/v2/plugins", None).await;
    assert_eq!(s, 200);
    assert!(body.is_array(), "插件列表（初始为空数组）");
}

#[tokio::test]
async fn tc_sv4_workers_preseeded() {
    let kernel = std::sync::Arc::new(RuntimeKernel::new(
        openaip_os::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap(),
    ));
    let addr = common::spawn_server(kernel).await;
    let (s, body) = call(addr, "GET", "/api/v1/v2/workers", None).await;
    assert_eq!(s, 200);
    let arr = body.as_array().unwrap();
    // 预置 4 个 Worker：echo / git / codex / claude-code
    let ids: Vec<&str> = arr.iter().map(|w| w["id"].as_str().unwrap()).collect();
    for expected in ["echo", "git", "codex", "claude-code"] {
        assert!(
            ids.contains(&expected),
            "预置 worker {expected} 缺失: {ids:?}"
        );
    }
    let codex = arr.iter().find(|w| w["id"] == "codex").unwrap();
    assert!(
        codex["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c == "code.write"),
        "codex 壳声明 code.write"
    );
}

#[tokio::test]
async fn tc_sv3_session_create_and_read() {
    let kernel = std::sync::Arc::new(RuntimeKernel::new(
        openaip_os::storage::LibSQLStorage::open(":memory:")
            .await
            .unwrap(),
    ));
    let addr = common::spawn_server(kernel).await;

    let (s1, body1) = call(addr, "POST", "/api/v1/v2/sessions/swe-1/create", None).await;
    assert_eq!(s1, 200);
    assert_eq!(body1["session_id"], "swe-1");
    assert_eq!(body1["events"][0]["event_type"], "session.created");

    let (s2, body2) = call(addr, "GET", "/api/v1/v2/sessions/swe-1", None).await;
    assert_eq!(s2, 200);
    assert_eq!(body2["events"].as_array().unwrap().len(), 1);

    // 不存在 → 404
    let (s3, _) = call(addr, "GET", "/api/v1/v2/sessions/nope", None).await;
    assert_eq!(s3, 404);
}

// ── SV5：Phase 11 评测端点（证据系统 DTO）───────────────────────────
#[tokio::test]
async fn tc_sv5_evaluation_endpoint() {
    let storage = openaip_os::storage::LibSQLStorage::open(":memory:")
        .await
        .unwrap();
    let kernel = std::sync::Arc::new(openaip_os::runtime::RuntimeKernel::new(storage.clone()));
    let addr = common::spawn_server(kernel).await;

    // 空列表
    let (s0, body0) = call(addr, "GET", "/api/v1/wb/agent/evaluation", None).await;
    assert_eq!(s0, 200);
    assert_eq!(body0["data"]["total"], 0);

    // 种子一条评测（RunEnd 钩子产物）
    let mut log = openaip_os::session::log::SessionLog::new(
        openaip_os::session::event::SessionId::new("run_e1"),
    );
    log.append(
        openaip_os::session::event::EventType::TestResult,
        json!({ "tests_total": 10, "tests_passed": 8 }),
    );
    log.append(
        openaip_os::session::event::EventType::PolicyChecked,
        json!({ "capability": "code.test", "verdict": "allow" }),
    );
    let task = openaip_os::evaluation::TaskDefinition {
        task_id: "t1".into(),
        expected_relations: vec![],
        expected_artifacts: vec![],
        time_budget_secs: 0.0,
        skill_version: None,
    };
    openaip_os::evaluation::hook::on_run_end(
        &mut log,
        "run_e1",
        &task,
        &openaip_os::evaluation::EvaluationPolicy::default(),
        &storage,
    )
    .await
    .unwrap();

    // 列表可见（派生表 DTO：value/confidence/evidence/unknowns）
    let (s1, body1) = call(addr, "GET", "/api/v1/wb/agent/evaluation", None).await;
    assert_eq!(s1, 200);
    assert_eq!(body1["data"]["total"], 1);
    assert_eq!(body1["data"]["list"][0]["runId"], "run_e1");
    assert_eq!(body1["data"]["list"][0]["correctness"]["value"], json!(0.8));
    assert_eq!(
        body1["data"]["list"][0]["correctness"]["evidence"][0]["eventType"],
        json!("test.result")
    );
    assert_eq!(
        body1["data"]["list"][0]["policyCompliance"]["value"],
        json!(1.0)
    );
    assert!(
        body1["data"]["list"][0]["unknowns"]
            .as_array()
            .unwrap()
            .contains(&json!("efficiency"))
    );

    // runId 过滤
    let (s2, body2) = call(
        addr,
        "GET",
        "/api/v1/wb/agent/evaluation?runId=run_e1",
        None,
    )
    .await;
    assert_eq!(s2, 200);
    assert_eq!(body2["data"]["total"], 1);
    let (s3, body3) = call(addr, "GET", "/api/v1/wb/agent/evaluation?runId=nope", None).await;
    assert_eq!(s3, 200);
    assert_eq!(body3["data"]["total"], 0);
}

// ── SV6：Phase 11B 诊断环端点（提案列表 + 人工评审 + Apply）─────────
#[tokio::test]
async fn tc_sv6_mutation_review_endpoint() {
    let storage = openaip_os::storage::LibSQLStorage::open(":memory:")
        .await
        .unwrap();
    let kernel = std::sync::Arc::new(openaip_os::runtime::RuntimeKernel::new(storage.clone()));
    let addr = common::spawn_server(kernel).await;

    let reducer = openaip_os::evaluation::EvaluationReducer::default();
    let task = openaip_os::evaluation::TaskDefinition {
        task_id: "fix-java-bug".into(),
        expected_relations: vec![],
        expected_artifacts: vec![],
        time_budget_secs: 10.0,
        skill_version: None,
    };
    // 种子 2 条评测：执行但无正确性证据 → T1 诊断
    for i in 0..2 {
        let mut log = openaip_os::session::log::SessionLog::new(
            openaip_os::session::event::SessionId::new(&format!("run_{i}")),
        );
        log.append(
            openaip_os::session::event::EventType::WorkerStarted,
            json!({ "worker": "w1" }),
        );
        log.append(
            openaip_os::session::event::EventType::WorkerCompleted,
            json!({ "worker": "w1", "duration_ms": 5000.0 }),
        );
        let r = reducer.evaluate(log.events(), &format!("run_{i}"), &task);
        storage.save_evaluation(&r).await.unwrap();
    }
    let proposals = openaip_os::run_diagnosis(
        &storage,
        "fix-java-bug",
        "v1",
        None,
        &openaip_os::DiagnosisConfig::default(),
    )
    .await
    .unwrap();
    assert_eq!(proposals.len(), 1, "T1 命中：执行无正确性证据");
    let pid = proposals[0].id.clone();

    // 列表可见（Operator 读）+ 新提案 = PROPOSED（等待人工评审）
    let (s0, body0) = call(addr, "GET", "/api/v1/wb/agent/mutation", None).await;
    assert_eq!(s0, 200);
    assert_eq!(body0["data"]["total"], 1);
    assert_eq!(body0["data"]["list"][0]["id"], json!(pid));
    assert_eq!(body0["data"]["list"][0]["status"], "PROPOSED");

    // 状态过滤
    let (s1, body1) = call(
        addr,
        "GET",
        "/api/v1/wb/agent/mutation?status=PROPOSED",
        None,
    )
    .await;
    assert_eq!(s1, 200);
    assert_eq!(body1["data"]["total"], 1);

    // 人工评审 approve → VALIDATED；Apply 因基准 Skill 不在 Registry 失败（提示人工细化，不影响评审）
    let (s2, body2) = call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/mutation/{pid}/review"),
        Some(json!({ "decision": "approve" })),
    )
    .await;
    assert_eq!(s2, 200, "评审: {body2}");
    assert_eq!(body2["data"]["status"], "VALIDATED");
    assert!(
        body2["data"]["promotionReason"]
            .as_str()
            .unwrap()
            .contains("apply failed")
    );

    // 已评审提案不被重新生成
    let rerun = openaip_os::run_diagnosis(
        &storage,
        "fix-java-bug",
        "v1",
        None,
        &openaip_os::DiagnosisConfig::default(),
    )
    .await
    .unwrap();
    assert!(rerun.is_empty(), "已评审提案不重复");

    // 第二条诊断场景：失败模式重复（T2）→ reject → REJECTED（失败保留）
    for i in 2..4 {
        let mut log = openaip_os::session::log::SessionLog::new(
            openaip_os::session::event::SessionId::new(&format!("run_{i}")),
        );
        log.append(
            openaip_os::session::event::EventType::WorkerStarted,
            json!({ "worker": "w1", "attempt": 1 }),
        );
        log.append(
            openaip_os::session::event::EventType::WorkerFailed,
            json!({ "worker": "w1", "error": "integration timeout" }),
        );
        let r = reducer.evaluate(log.events(), &format!("run_{i}"), &task);
        storage.save_evaluation(&r).await.unwrap();
    }
    let proposals2 = openaip_os::run_diagnosis(
        &storage,
        "fix-java-bug",
        "v1",
        None,
        &openaip_os::DiagnosisConfig::default(),
    )
    .await
    .unwrap();
    let t2 = proposals2
        .iter()
        .find(|p| p.id.contains("integration-timeout"))
        .expect("T2 命中");
    let (s3, body3) = call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/mutation/{}/review", t2.id),
        Some(json!({ "decision": "reject", "reason": "draft 步骤需细化" })),
    )
    .await;
    assert_eq!(s3, 200, "评审: {body3}");
    assert_eq!(body3["data"]["status"], "REJECTED");
    assert!(
        body3["data"]["promotionReason"]
            .as_str()
            .unwrap()
            .contains("draft 步骤需细化")
    );
}

// ── SV7：Phase 11C 离线 Benchmark 端点 ───────────────────────────────
#[tokio::test]
async fn tc_sv7_offline_benchmark_endpoint() {
    let storage = openaip_os::storage::LibSQLStorage::open(":memory:")
        .await
        .unwrap();
    let kernel = std::sync::Arc::new(openaip_os::runtime::RuntimeKernel::new(storage.clone()));
    let addr = common::spawn_server(kernel).await;

    let reducer = openaip_os::evaluation::EvaluationReducer::default();
    let task = openaip_os::evaluation::TaskDefinition {
        task_id: "fix-java-bug".into(),
        expected_relations: vec![],
        expected_artifacts: vec![],
        time_budget_secs: 10.0,
        skill_version: None,
    };
    // 5 个历史 run：执行但无正确性证据（T1 模式）
    for i in 0..5 {
        let mut log = openaip_os::session::log::SessionLog::new(
            openaip_os::session::event::SessionId::new(&format!("run_{i}")),
        );
        log.append(
            openaip_os::session::event::EventType::WorkerStarted,
            json!({ "worker": "w1", "attempt": 1 }),
        );
        log.append(
            openaip_os::session::event::EventType::WorkerCompleted,
            json!({ "worker": "w1", "ok": true, "duration_ms": 5000.0 }),
        );
        let r = reducer.evaluate(log.events(), &format!("run_{i}"), &task);
        storage.save_evaluation(&r).await.unwrap();
    }
    // 任务定义持久化（hook 语义：run_end 落 wb_task）
    storage.save_task(&task).await.unwrap();
    assert!(storage.get_task("fix-java-bug").await.unwrap().is_some());

    // 诊断 → 提案 → 人工评审 approve → VALIDATED
    let proposals = openaip_os::run_diagnosis(
        &storage,
        "fix-java-bug",
        "v1",
        None,
        &openaip_os::DiagnosisConfig::default(),
    )
    .await
    .unwrap();
    let pid = proposals[0].id.clone();
    let (s0, _) = call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/mutation/{pid}/review"),
        Some(json!({ "decision": "approve" })),
    )
    .await;
    assert_eq!(s0, 200);

    // 离线 Benchmark：覆盖率 1.0 → pass → EXPERIMENTAL + verdict
    let (s1, body1) = call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/mutation/{pid}/offline-benchmark"),
        None,
    )
    .await;
    assert_eq!(s1, 200, "benchmark: {body1}");
    assert_eq!(body1["data"]["status"], "EXPERIMENTAL");
    assert_eq!(body1["data"]["verdict"]["pass"], json!(true));
    assert_eq!(body1["data"]["verdict"]["baselineRuns"], 5);
    assert_eq!(body1["data"]["verdict"]["coverage"], json!(1.0));
    assert!(
        body1["data"]["promotionReason"]
            .as_str()
            .unwrap()
            .contains("offline pass")
    );

    // 失败路径：任务集过滤为空 → 数据不足 → REJECTED（持久）
    let mut p2 = openaip_os::mutation::MutationProposal::propose(
        "fix-java-bug",
        "v1",
        "v2",
        "rc",
        vec!["run_0".into()],
        json!({ "insert_step": { "step": "run_tests" } }),
    )
    .unwrap();
    p2.validate().unwrap();
    storage.save_mutation(&p2).await.unwrap();
    let (s2, body2) = call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/mutation/{}/offline-benchmark", p2.id),
        Some(json!({ "taskIds": ["other-task"] })),
    )
    .await;
    assert_eq!(s2, 200, "benchmark: {body2}");
    assert_eq!(body2["data"]["status"], "REJECTED");
    assert_eq!(body2["data"]["verdict"]["pass"], json!(false));

    // REJECTED 不可再 Benchmark
    let (s3, body3) = call(
        addr,
        "POST",
        &format!("/api/v1/wb/agent/mutation/{}/offline-benchmark", p2.id),
        None,
    )
    .await;
    assert_eq!(s3, 200, "WB 错误码在 body 中");
    assert_eq!(body3["code"], 4001, "REJECTED 不可复活: {body3}");
}

// ── SV8：Loop Health 端点（四平面六指标派生）────────────────────────
#[tokio::test]
async fn tc_sv8_loop_health_endpoint() {
    let storage = openaip_os::storage::LibSQLStorage::open(":memory:")
        .await
        .unwrap();
    let kernel = std::sync::Arc::new(openaip_os::runtime::RuntimeKernel::new(storage.clone()));
    let addr = common::spawn_server(kernel).await;

    // 种子：1 run + 1 评测（skill_version 基线）→ closure=1.0
    storage
        .insert_run("run_h1", "agent-1", "goal")
        .await
        .unwrap();
    let reducer = openaip_os::evaluation::EvaluationReducer::default();
    let task = openaip_os::evaluation::TaskDefinition {
        task_id: "t1".into(),
        expected_relations: vec![],
        expected_artifacts: vec![],
        time_budget_secs: 10.0,
        skill_version: Some("fix@v1".into()),
    };
    let mut log = openaip_os::session::log::SessionLog::new(
        openaip_os::session::event::SessionId::new("run_h1"),
    );
    log.append(
        openaip_os::session::event::EventType::WorkerStarted,
        json!({ "worker": "w1", "attempt": 1 }),
    );
    log.append(
        openaip_os::session::event::EventType::WorkerCompleted,
        json!({ "worker": "w1", "ok": true, "duration_ms": 1000.0 }),
    );
    let r = reducer.evaluate(log.events(), "run_h1", &task);
    storage.save_evaluation(&r).await.unwrap();

    let (s0, body0) = call(addr, "GET", "/api/v1/wb/agent/health", None).await;
    assert_eq!(s0, 200, "health: {body0}");
    assert_eq!(body0["data"]["execution"]["totalRuns"], 1);
    assert_eq!(
        body0["data"]["execution"]["closureRate"],
        json!(1.0),
        "1 run 全评测"
    );
    assert!(
        body0["data"]["observation"]["evaluationCoverage"]
            .as_f64()
            .unwrap()
            > 0.0
    );
    assert_eq!(body0["data"]["evolution"]["experimentalOrActive"], 0);
    assert_eq!(
        body0["data"]["governance"]["policyChecks"], 0,
        "无 trace 事件 → 0"
    );
}
