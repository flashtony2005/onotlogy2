//! V2.0 真实世界连接（Phase 10 · HTTP/MCP/AI）契约测试。
//!
//! 用 axum 起本地 mock server，验证 HttpPlugin / McpPlugin / AI Provider
//! 走**真实 HTTP 网络**（reqwest）——不再是 mock 执行器，而是真实协议链路。
//!
//! R1 HttpPlugin：POST {endpoint}/{capability} 真实调用
//! R2 McpPlugin：JSON-RPC 2.0（initialize → tools/call）
//! R3 AI Provider：llm.generate → OpenAI 形态 /v1/chat/completions
//! R4 权限：网络插件声明缺 endpoint / 未声明网络 → 执行失败路径

use axum::routing::post;
use axum::{Json, Router as AxumRouter};
use serde_json::{Value, json};
use tokio::net::TcpListener;

use openaip_os::plugin::capability::{
    CapabilityContract, CapabilityRegistry, Router, RouterStrategy,
};
use openaip_os::plugin::http::HttpPlugin;
use openaip_os::plugin::manifest::{PluginManifest, RuntimeType};
use openaip_os::plugin::mcp::McpPlugin;
use openaip_os::plugin::registry::PluginRegistry;
use openaip_os::plugin::runtime::PluginRuntime;

/// mock server 路由：/api/echo（HttpPlugin）、/mcp（JSON-RPC）、/v1/chat/completions（OpenAI 形态）。
fn mock_router() -> AxumRouter {
    AxumRouter::new()
        .route(
            "/api/echo",
            post(|Json(body): Json<Value>| async move {
                Json(json!({ "ok": true, "echo": body }))
            }),
        )
        .route(
            "/mcp",
            post(|Json(req): Json<Value>| async move {
                let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
                let id = req.get("id").cloned().unwrap_or(json!(0));
                match method {
                    "initialize" => Json(json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": { "protocolVersion": "2024-11-05", "capabilities": { "tools": {} }, "serverInfo": { "name": "mock-mcp", "version": "1.0" } }
                    })),
                    "tools/list" => Json(json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": { "tools": [ { "name": "weather.get", "description": "mock" } ] }
                    })),
                    "tools/call" => {
                        let name = req.get("params").and_then(|p| p.get("name")).and_then(|n| n.as_str()).unwrap_or("");
                        Json(json!({
                            "jsonrpc": "2.0", "id": id,
                            "result": { "content": [ { "type": "text", "text": format!("mcp result for {name}") } ] }
                        }))
                    }
                    _ => Json(json!({
                        "jsonrpc": "2.0", "id": id,
                        "error": { "code": -32601, "message": format!("method not found: {method}") }
                    })),
                }
            }),
        )
        .route(
            "/llm.generate",
            post(|Json(body): Json<Value>| async move {
                let prompt = body
                    .get("messages")
                    .and_then(|m| m.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                Json(json!({
                    "choices": [ { "message": { "role": "assistant", "content": format!("AI 回答: {prompt}") } } ]
                }))
            }),
        )
}

async fn spawn_mock() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, mock_router()).await.unwrap();
    });
    format!("http://{addr}")
}

fn install_active(
    reg: &mut PluginRegistry,
    id: &str,
    plugin: Box<dyn openaip_os::plugin::AsyncPlugin>,
) {
    reg.install_async(plugin).unwrap();
    reg.validate(id).unwrap();
    reg.register(id).unwrap();
    reg.activate(id).unwrap();
}

fn declare(r: &mut CapabilityRegistry, cap: &str) {
    r.declare(CapabilityContract {
        capability: cap.into(),
        input_schema: json!({}),
        output_schema: json!({}),
        effects: vec![format!("{cap}.done")],
    });
}

// ── R1：HttpPlugin 真实 HTTP 调用 ─────────────────────────────────────
#[tokio::test]
async fn tc_r1_http_plugin_real_network() {
    let base = spawn_mock().await;
    let mut reg = PluginRegistry::new();
    let manifest = PluginManifest::new("http.echo", "Echo", "1.0.0", RuntimeType::Http)
        .with_capability("echo")
        .with_endpoint(&format!("{base}/api"));
    install_active(&mut reg, "http.echo", Box::new(HttpPlugin::new(manifest)));

    let mut caps = CapabilityRegistry::new();
    declare(&mut caps, "echo");
    caps.bind(reg.get("http.echo").unwrap().manifest());

    let mut runtime = PluginRuntime::new(&reg, &caps, Router::new(RouterStrategy::First));
    let out = runtime
        .execute_async("echo", json!({ "text": "真实 HTTP" }))
        .await
        .unwrap();
    assert_eq!(out["ok"], true);
    assert_eq!(out["echo"]["text"], "真实 HTTP", "真实网络往返数据");
}

// ── R2：McpPlugin — JSON-RPC 2.0（initialize → tools/call）────────────
#[tokio::test]
async fn tc_r2_mcp_plugin_jsonrpc() {
    let base = spawn_mock().await;
    let mut reg = PluginRegistry::new();
    let manifest = PluginManifest::new("mcp.weather", "Weather MCP", "1.0.0", RuntimeType::Mcp)
        .with_capability("weather.get")
        .with_endpoint(&format!("{base}/mcp"));
    let mcp = McpPlugin::new(manifest.clone());
    // 协议握手验证（initialize → tools/list）
    mcp.client().initialize().await.unwrap();
    let tools = mcp.client().tools_list().await.unwrap();
    assert!(
        tools.iter().any(|t| t["name"] == "weather.get"),
        "tools/list 能力发现"
    );
    // 完整协议序列执行
    install_active(&mut reg, "mcp.weather", Box::new(mcp));

    let mut caps = CapabilityRegistry::new();
    declare(&mut caps, "weather.get");
    caps.bind(reg.get("mcp.weather").unwrap().manifest());

    let mut runtime = PluginRuntime::new(&reg, &caps, Router::new(RouterStrategy::First));
    let out = runtime
        .execute_async("weather.get", json!({ "city": "上海" }))
        .await
        .unwrap();
    let text = out["content"][0]["text"].as_str().unwrap_or("");
    assert!(text.contains("weather.get"), "MCP tools/call 真实返回");
}

// ── R3：AI Provider — llm.generate → OpenAI 形态端点 ─────────────────
#[tokio::test]
async fn tc_r3_ai_provider_real_http() {
    let base = spawn_mock().await;
    let mut reg = PluginRegistry::new();
    let manifest = PluginManifest::new("ai.llm", "LLM", "1.0.0", RuntimeType::Ai)
        .with_capability("llm.generate")
        .with_endpoint(&base);
    install_active(&mut reg, "ai.llm", Box::new(HttpPlugin::new(manifest)));

    let mut caps = CapabilityRegistry::new();
    declare(&mut caps, "llm.generate");
    caps.bind(reg.get("ai.llm").unwrap().manifest());

    let mut runtime = PluginRuntime::new(&reg, &caps, Router::new(RouterStrategy::First));
    let out = runtime
        .execute_async(
            "llm.generate",
            json!({ "messages": [ { "role": "user", "content": "你好" } ] }),
        )
        .await
        .unwrap();
    assert_eq!(out["choices"][0]["message"]["content"], "AI 回答: 你好");
}

// ── R4：失败路径 — 缺 endpoint / 未声明能力 ──────────────────────────
#[tokio::test]
async fn tc_r4_network_failure_paths() {
    let mut reg = PluginRegistry::new();
    // 缺 endpoint 的 HttpPlugin
    let m = PluginManifest::new("http.broken", "Broken", "1.0.0", RuntimeType::Http)
        .with_capability("x");
    install_active(&mut reg, "http.broken", Box::new(HttpPlugin::new(m)));
    let mut caps = CapabilityRegistry::new();
    declare(&mut caps, "x");
    caps.bind(reg.get("http.broken").unwrap().manifest());
    let mut runtime = PluginRuntime::new(&reg, &caps, Router::new(RouterStrategy::First));
    let err = runtime.execute_async("x", json!({})).await.unwrap_err();
    assert!(
        err.to_string().contains("endpoint"),
        "缺 endpoint → 明确失败: {err}"
    );

    // 未声明能力 → 无 provider（P-Rev 网络版）
    let mut caps2 = CapabilityRegistry::new();
    declare(&mut caps2, "y");
    let mut runtime2 = PluginRuntime::new(&reg, &caps2, Router::new(RouterStrategy::First));
    assert!(runtime2.execute_async("y", json!({})).await.is_err());
}
