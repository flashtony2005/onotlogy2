//! 知识本体 HTTP 契约：DL 推理、版本治理、人工评审、应用与回滚。

mod common;

use openaip_os::auth::{IdentityProvider, LocalIdentityProvider};
use openaip_os::cognitive::engine::PlanStep;
use openaip_os::cognitive::{CognitiveEngine, FakeEngine};
use openaip_os::server;
use openaip_os::storage::{LibSQLStorage, ObjectEngine, UserRecord, WorldCommand};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

async fn call(addr: SocketAddr, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let client = reqwest::Client::new();
    let mut request = client.request(
        reqwest::Method::from_bytes(method.as_bytes()).unwrap(),
        format!("http://{addr}{path}"),
    );
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap_or(json!(null)))
}

fn class(name: &str) -> Value {
    json!({ "Class": name })
}

fn subclass(sub: &str, sup: &str) -> Value {
    json!({ "SubClassOf": { "sub": class(sub), "sup": class(sup) } })
}

fn root_ontology() -> Value {
    json!({ "axioms": [subclass("Dog", "Animal")] })
}

fn disjoint_customer_order_ontology() -> Value {
    json!({ "axioms": [
        subclass("Customer", "Thing"),
        subclass("Order", "Thing"),
        { "DisjointClasses": [class("Customer"), class("Order")] },
        { "DataPropertyDomain": { "property": "age", "class": class("Customer") } }
    ] })
}

fn runtime_ir(refund_action: bool) -> Value {
    let mut actions = Vec::new();
    if refund_action {
        actions.push(json!({
            "name": "refund",
            "params": [],
            "creates": [{
                "object_type": "RefundTask",
                "link_edge": "refunded"
            }]
        }));
    }
    json!({
        "object_types": [
            {
                "name": "Customer",
                "properties": [
                    { "name": "name", "ty": "String" },
                    { "name": "birthday", "ty": "Date" },
                    { "name": "balance", "ty": "Decimal" }
                ],
                "actions": actions
            },
            {
                "name": "Order",
                "properties": [{ "name": "total", "ty": "Int" }],
                "actions": []
            },
            { "name": "RefundTask", "properties": [], "actions": [] }
        ],
        "relations": [
            { "name": "placed", "source": "Customer", "target": "Order" },
            { "name": "refunded", "source": "Customer", "target": "RefundTask" }
        ]
    })
}

#[tokio::test]
async fn knowledge_http_supports_reasoning_governed_evolution_and_rollback() {
    let kernel = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    seed_world(kernel.as_ref()).await;
    let addr = common::spawn_server(kernel).await;

    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/ontology/init",
        Some(json!({
            "id": "root",
            "ontology": root_ontology(),
            "runtimeIr": runtime_ir(false)
        })),
    )
    .await;
    assert_eq!(status, 200, "初始化失败: {body}");
    assert_eq!(body["code"], 0);
    let root_persisted_ontology = body["data"]["ontology"].clone();
    assert_ne!(
        root_persisted_ontology,
        root_ontology(),
        "runtime IR 必须物化进 DL TBox"
    );

    let (status, body) = call(addr, "GET", "/api/v1/knowledge/diagnosis", None).await;
    assert_eq!(status, 200, "健康诊断失败: {body}");
    assert_eq!(body["data"]["consistent"], true);
    assert_eq!(body["data"]["unsatisfiableClasses"], json!([]));

    let bad_runtime = json!({
        "object_types": [{
            "name": "Customer",
            "properties": [{ "name": "name", "ty": "Int" }],
            "actions": []
        }]
    });
    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/proposals",
        Some(json!({
            "sourceVersionId": "root",
            "proposal": { "id": "bad-runtime", "changes": [] },
            "runtimeIr": bad_runtime
        })),
    )
    .await;
    assert_eq!(status, 400, "不兼容运行时本体必须失败关闭: {body}");

    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/proposals",
        Some(json!({
            "sourceVersionId": "root",
            "proposal": {
                "id": "add-puppy",
                "changes": [{ "Add": subclass("Puppy", "Dog") }]
            },
            "runtimeIr": runtime_ir(false)
        })),
    )
    .await;
    assert_eq!(status, 200, "创建提案失败: {body}");
    assert_eq!(body["data"]["review"]["decision"], "Pending");

    let (status, _) = call(addr, "GET", "/api/v1/knowledge/reason", None).await;
    assert_eq!(status, 405, "reason 是 POST 端点");

    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/reason",
        Some(json!({ "query": "consistency" })),
    )
    .await;
    assert_eq!(status, 200, "推理失败: {body}");
    assert_eq!(body["data"]["consistent"], true);

    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/reason",
        Some(json!({
            "query": "isInstanceOf",
            "class": "Dog",
            "individual": "dog-1"
        })),
    )
    .await;
    assert_eq!(status, 200, "实例检查失败: {body}");
    assert_eq!(body["data"]["instance"], true);

    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/reason",
        Some(json!({
            "query": "entailsObjectProperty",
            "property": "placed",
            "subject": "customer-1",
            "object": "order-1"
        })),
    )
    .await;
    assert_eq!(status, 200, "关系断言检查失败: {body}");
    assert_eq!(body["data"]["related"], true);

    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/reason",
        Some(json!({
            "query": "dataProperty",
            "property": "name",
            "individual": "customer-1",
            "value": { "String": "Ada" }
        })),
    )
    .await;
    assert_eq!(status, 200, "数据属性蕴含检查失败: {body}");
    assert_eq!(body["data"]["entailed"], true);

    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/reason",
        Some(json!({
            "query": "dataProperty",
            "property": "birthday",
            "individual": "customer-1",
            "value": { "Date": "2024-02-29" }
        })),
    )
    .await;
    assert_eq!(status, 200, "日期属性蕴含检查失败: {body}");
    assert_eq!(body["data"]["entailed"], true);

    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/reason",
        Some(json!({
            "query": "dataProperty",
            "property": "balance",
            "individual": "customer-1",
            "value": { "Decimal": "123.45" }
        })),
    )
    .await;
    assert_eq!(status, 200, "exact decimal entailment failed: {body}");
    assert_eq!(body["data"]["entailed"], true);

    for (query, extra, expected_justification) in [
        (
            "explainSubclass",
            json!({ "sub": "Dog", "sup": "Animal" }),
            true,
        ),
        (
            "explainObjectProperty",
            json!({
                "property": "placed",
                "subject": "customer-1",
                "object": "order-1"
            }),
            true,
        ),
        (
            "explainDataProperty",
            json!({
                "property": "name",
                "individual": "customer-1",
                "value": { "String": "Ada" }
            }),
            true,
        ),
        (
            "explainInstance",
            json!({ "class": "Dog", "individual": "dog-1" }),
            true,
        ),
        ("explainUnsatisfiable", json!({ "class": "Dog" }), false),
        ("explainInconsistency", json!({}), false),
    ] {
        let mut payload = json!({ "query": query });
        for (key, value) in extra.as_object().unwrap() {
            payload[key] = value.clone();
        }
        let (status, body) = call(addr, "POST", "/api/v1/knowledge/reason", Some(payload)).await;
        assert_eq!(status, 200, "{query} 失败: {body}");
        if expected_justification {
            assert!(
                body["data"]["justification"].is_object(),
                "{query} 应返回 justification: {body}"
            );
        } else {
            assert_eq!(body["data"]["justification"], json!(null));
        }
    }

    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/proposals/add-puppy/review",
        Some(json!({ "approve": true })),
    )
    .await;
    assert_eq!(status, 200, "评审失败: {body}");
    assert_eq!(body["data"]["review"]["decision"], "Approved");
    assert_eq!(body["data"]["review"]["reviewer"], "test");

    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/proposals/add-puppy/apply",
        Some(json!({ "targetVersionId": "v2" })),
    )
    .await;
    assert_eq!(status, 200, "应用失败: {body}");
    assert_eq!(body["data"]["version"]["id"], "v2");

    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/reason",
        Some(json!({
            "query": "entailsSubclass",
            "sub": "Puppy",
            "sup": "Animal"
        })),
    )
    .await;
    assert_eq!(status, 200, "传递子类推理失败: {body}");
    assert_eq!(body["data"]["entailed"], true);

    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/rollback",
        Some(json!({
            "proposalId": "restore-root",
            "targetVersionId": "root"
        })),
    )
    .await;
    assert_eq!(status, 200, "创建回滚失败: {body}");

    call(
        addr,
        "POST",
        "/api/v1/knowledge/proposals/restore-root/review",
        Some(json!({ "approve": true, "reason": "restore baseline" })),
    )
    .await;
    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/proposals/restore-root/apply",
        Some(json!({ "targetVersionId": "v3" })),
    )
    .await;
    assert_eq!(status, 200, "应用回滚失败: {body}");

    let (status, body) = call(addr, "GET", "/api/v1/knowledge/ontology/current", None).await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["id"], "v3");
}

#[tokio::test]
async fn knowledge_diagnosis_explains_live_world_inconsistency() {
    let kernel = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    let addr = common::spawn_server(kernel.clone()).await;

    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/ontology/init",
        Some(json!({
            "id": "root",
            "ontology": disjoint_customer_order_ontology(),
            "runtimeIr": null
        })),
    )
    .await;
    assert_eq!(status, 200, "初始化失败: {body}");

    // World writes remain available after approval; diagnosis is the read-only
    // gate that can detect facts which invalidate the current knowledge base.
    kernel
        .create("Customer", "customer-1", json!({}))
        .await
        .unwrap();
    kernel
        .create("Order", "order-1", json!({ "age": 30 }))
        .await
        .unwrap();

    let (status, body) = call(addr, "GET", "/api/v1/knowledge/diagnosis", None).await;
    assert_eq!(status, 200, "诊断失败: {body}");
    assert_eq!(body["data"]["consistent"], false);
    assert!(body["data"]["inconsistency"]["axioms"].is_array());
    assert!(
        !body["data"]["inconsistency"]["axioms"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

async fn seed_world(kernel: &LibSQLStorage) {
    kernel
        .create(
            "Customer",
            "customer-1",
            json!({ "name": "Ada", "birthday": "2024-02-29", "balance": "123.4500" }),
        )
        .await
        .unwrap();
    kernel
        .create("Dog", "dog-1", json!({ "name": "Rex" }))
        .await
        .unwrap();
    kernel
        .create("Order", "order-1", json!({ "total": 0 }))
        .await
        .unwrap();
    kernel
        .apply_world(WorldCommand::LinkRelation {
            source: "customer-1".to_owned(),
            edge: "placed".to_owned(),
            target: "order-1".to_owned(),
            project_id: None,
        })
        .await
        .unwrap();
}

fn refund_engine() -> Arc<dyn CognitiveEngine> {
    Arc::new(FakeEngine::new(vec![PlanStep {
        capability: "Customer.refund".into(),
        args: json!({ "target": "customer-1" }),
        when: Some(Box::new(|view| {
            !view
                .related
                .iter()
                .any(|(_, predicate, _)| predicate == "refunded")
        })),
    }]))
}

async fn spawn_custom(
    kernel: Arc<LibSQLStorage>,
    services: Vec<openaip_os::compose::Service>,
    defs: Vec<openaip_os::compose::defs::RuntimeDefinition>,
    engine: Arc<dyn CognitiveEngine>,
) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server::serve(
            kernel, services, defs, engine, listener, "ui/dist", false, false, false,
        )
        .await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    addr
}

async fn wait_run_completed(kernel: &LibSQLStorage, run_id: &str) -> bool {
    for _ in 0..100 {
        let completed = kernel
            .list_runs()
            .await
            .unwrap()
            .into_iter()
            .any(|run| run.id == run_id && run.status == "Completed");
        if completed {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    false
}

#[tokio::test]
async fn knowledge_diagnosis_reports_closed_world_cardinality_violations() {
    let kernel = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    for (object_type, id) in [
        ("Customer", "customer-1"),
        ("Order", "order-1"),
        ("Order", "order-2"),
    ] {
        kernel.create(object_type, id, json!({})).await.unwrap();
    }
    kernel
        .apply_world(WorldCommand::LinkRelation {
            source: "customer-1".into(),
            edge: "placed".into(),
            target: "order-1".into(),
            project_id: None,
        })
        .await
        .unwrap();

    let mut candidate: openaip_os::abi::ir::OntologyIR =
        serde_json::from_value(runtime_ir(false)).unwrap();
    let placed = candidate
        .relations
        .iter_mut()
        .find(|relation| relation.name == "placed")
        .unwrap();
    placed.min_cardinality = Some(1);
    placed.max_cardinality = Some(2);
    kernel
        .initialize_knowledge_version(
            "root",
            &serde_json::from_value(root_ontology()).unwrap(),
            Some(&candidate),
        )
        .await
        .unwrap();

    let addr = common::spawn_server(kernel.clone()).await;
    let (status, body) = call(addr, "GET", "/api/v1/knowledge/diagnosis", None).await;
    assert_eq!(status, 200, "初始化后的诊断失败: {body}");
    assert_eq!(body["data"]["consistent"], true);
    assert_eq!(body["data"]["cardinalityViolations"], json!([]));

    // World evolution after approval must remain diagnosable. A missing minimum
    // edge is a closed-world executable-schema violation, not an OWL inconsistency.
    kernel
        .apply_world(WorldCommand::UnlinkRelation {
            source: "customer-1".into(),
            edge: "placed".into(),
            target: "order-1".into(),
            project_id: None,
        })
        .await
        .unwrap();

    let (status, body) = call(addr, "GET", "/api/v1/knowledge/diagnosis", None).await;
    assert_eq!(status, 200, "基数诊断失败: {body}");
    assert_eq!(body["data"]["consistent"], true);
    let violations = body["data"]["cardinalityViolations"].as_array().unwrap();
    assert_eq!(1, violations.len());
    assert_eq!("customer-1", violations[0]["individual"]);
    assert_eq!("placed", violations[0]["property"]);
    assert_eq!(1, violations[0]["min"]);
    assert_eq!(0, violations[0]["actual"]);

    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/reason",
        Some(json!({ "query": "consistency" })),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["consistent"], true);
}

#[tokio::test]
async fn approved_runtime_evolution_hot_swaps_compiled_definitions() {
    let kernel = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    kernel
        .create("Customer", "customer-1", json!({ "name": "Ada" }))
        .await
        .unwrap();
    let addr = spawn_custom(
        kernel.clone(),
        common::retain_services(),
        common::retain_defs(),
        refund_engine(),
    )
    .await;

    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/ontology/init",
        Some(json!({
            "id": "root",
            "ontology": root_ontology(),
            "runtimeIr": runtime_ir(false)
        })),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/proposals",
        Some(json!({
            "sourceVersionId": "root",
            "proposal": { "id": "add-refund", "changes": [] },
            "runtimeIr": runtime_ir(true)
        })),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    call(
        addr,
        "POST",
        "/api/v1/knowledge/proposals/add-refund/review",
        Some(json!({ "approve": true })),
    )
    .await;
    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/knowledge/proposals/add-refund/apply",
        Some(json!({ "targetVersionId": "v2" })),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/wb/agent/run",
        Some(json!({ "agentId": "refund-agent", "goal": "refund customer" })),
    )
    .await;
    assert_eq!(status, 200, "热更新后运行失败: {body}");
    let run_id = body["data"]["runId"].as_str().unwrap();
    assert!(wait_run_completed(&kernel, run_id).await);

    let events = kernel.list_all_events().await.unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "RelationLinked"
                && event.input.as_ref().and_then(|input| input.get("edge"))
                    == Some(&json!("refunded")))
    );
}

#[tokio::test]
async fn approved_runtime_evolution_survives_restart_as_compiled_definitions() {
    let kernel = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    kernel
        .create("Customer", "customer-1", json!({ "name": "Ada" }))
        .await
        .unwrap();

    // Apply through persisted governance state, then expose that database to a
    // fresh AppState whose initially compiled definitions are intentionally empty.
    kernel
        .initialize_knowledge_version(
            "root",
            &serde_json::from_value(root_ontology()).unwrap(),
            None,
        )
        .await
        .unwrap();
    let candidate: openaip_os::abi::ir::OntologyIR =
        serde_json::from_value(runtime_ir(true)).unwrap();
    let proposal = openaip_os::knowledge::OntologyProposal::new("add-refund");
    kernel
        .propose_knowledge_change("root", &proposal, Some(&candidate))
        .await
        .unwrap();
    kernel
        .decide_knowledge_proposal("add-refund", true, "alice", None)
        .await
        .unwrap();
    kernel
        .apply_knowledge_proposal("add-refund", "v2")
        .await
        .unwrap();

    let addr = spawn_custom(kernel.clone(), Vec::new(), Vec::new(), refund_engine()).await;
    let (status, body) = call(
        addr,
        "POST",
        "/api/v1/wb/agent/run",
        Some(json!({ "agentId": "refund-agent", "goal": "refund customer" })),
    )
    .await;
    assert_eq!(status, 200, "重启后运行失败: {body}");
    let run_id = body["data"]["runId"].as_str().unwrap();
    assert!(wait_run_completed(&kernel, run_id).await);

    let events = kernel.list_all_events().await.unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "RelationLinked"
                && event.input.as_ref().and_then(|input| input.get("edge"))
                    == Some(&json!("refunded")))
    );
}

async fn login(addr: SocketAddr, username: &str, password: &str) -> Option<String> {
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{addr}/api/v1/wb/auth/login"))
        .json(&json!({ "username": username, "password": password }))
        .send()
        .await
        .unwrap();
    let body: Value = response.json().await.unwrap();
    body["data"]["token"].as_str().map(str::to_owned)
}

#[tokio::test]
async fn executive_cannot_read_or_mutate_knowledge_governance() {
    let storage = LibSQLStorage::open(":memory:").await.unwrap();
    let idp = LocalIdentityProvider::new(storage.clone());
    idp.bootstrap_admin("admin", "adminpw").await.unwrap();
    let hash = LocalIdentityProvider::hash_password("execpw").unwrap();
    storage
        .create_user(&UserRecord {
            id: "user.exec".into(),
            username: "exec".into(),
            password_hash: hash,
            role: "executive".into(),
            display_name: "Executive".into(),
            disabled: false,
        })
        .await
        .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = server::serve_with_timeout(
            Arc::new(storage),
            Vec::new(),
            Vec::new(),
            Arc::new(openaip_os::cognitive::NoOpEngine),
            listener,
            "ui/dist",
            false,
            false,
            true,
            None,
        )
        .await;
    });

    let token = login(addr, "exec", "execpw").await.unwrap();
    let client = reqwest::Client::new();
    for (method, path) in [
        ("GET", "/api/v1/knowledge/ontology/current"),
        ("GET", "/api/v1/knowledge/diagnosis"),
        ("POST", "/api/v1/knowledge/ontology/init"),
        ("POST", "/api/v1/knowledge/proposals"),
        ("POST", "/api/v1/knowledge/rollback"),
    ] {
        let mut request = client.request(
            reqwest::Method::from_bytes(method.as_bytes()).unwrap(),
            format!("http://{addr}{path}"),
        );
        let payload = match method {
            "POST" if path.ends_with("/init") => {
                json!({ "id": "root", "ontology": root_ontology() })
            }
            "POST" if path.ends_with("/proposals") => json!({
                "sourceVersionId": "root",
                "proposal": { "id": "exec-proposal", "changes": [] }
            }),
            "POST" if path.ends_with("/rollback") => json!({
                "proposalId": "rollback",
                "targetVersionId": "root"
            }),
            _ => json!({}),
        };
        if method == "POST" {
            request = request.json(&payload);
        }
        let response = request
            .header("authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 403, "{method} {path}");
    }
}
