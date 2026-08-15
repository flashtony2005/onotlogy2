//! V2.0 Plugin & Capability Runtime（Phase 10）契约测试。
//!
//! TDD：先写测试（红），再实现（绿）。
//!
//! P1 Manifest 解析 + 序列化 roundtrip
//! P2 Plugin 安装 + Lifecycle（INSTALLED→VALIDATED→REGISTERED→ACTIVE→DISABLED→UNINSTALLED）
//! P3 Capability Binding：plugin 安装自动注册提供能力
//! P4 Router：多 provider 选择（explicit/first/best_effort）
//! P5 Runtime 执行：Local Function 真实执行 + 审计事件
//! P6 闭环：Skill requires → Capability → Plugin → 执行
//! P7 权限：请求超出 manifest 声明 → DENY（fail-closed）；secret 不进日志
//! P8 无实现能力 → 不可执行（UNRESOLVED）
//! P9 YouTube MVP：research/script/image/video/voice/subtitle 全链路路由+执行
//! P-Rev 反向：Plugin 不能提供未声明的 Capability；Skill 不能直接绑定 Provider

use openaip_os::plugin::capability::{
    CapabilityContract, CapabilityRegistry, ProviderBinding, Router, RouterStrategy,
};
use openaip_os::plugin::exec::LocalFunctionPlugin;
use openaip_os::plugin::manifest::{ConfigField, Permissions, PluginManifest, RuntimeType};
use openaip_os::plugin::registry::{PluginRegistry, PluginStatus};
use openaip_os::plugin::runtime::{PermissionRequest, PluginRuntime, RuntimeError};
use openaip_os::plugin::{Plugin, PluginError};
use openaip_os::session::event::SessionId;
use openaip_os::session::log::SessionLog;
use serde_json::{Value, json};

/// 构造一个 Local Function 插件（capability: file.read）。
fn file_read_plugin() -> LocalFunctionPlugin {
    let manifest = PluginManifest::new(
        "local.filesystem",
        "Filesystem Plugin",
        "1.0.0",
        RuntimeType::Local,
    )
    .with_capability("file.read")
    .with_permissions(Permissions {
        filesystem_read: vec!["/workspace/input".into()],
        ..Default::default()
    });
    LocalFunctionPlugin::new(
        manifest,
        Box::new(|_cap, input| {
            let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            Ok(json!({ "content": format!("content of {path}") }))
        }),
    )
}

/// 构造一个 AI 插件（capability: llm.generate；声明 network + secret）。
fn llm_plugin() -> LocalFunctionPlugin {
    let manifest = PluginManifest::new("ai.llm", "LLM Plugin", "1.0.0", RuntimeType::Ai)
        .with_capability("llm.generate")
        .with_config(ConfigField {
            name: "api_key".into(),
            secret: true,
        })
        .with_permissions(Permissions {
            network: vec!["api.openai.com".into()],
            secrets: vec!["api_key".into()],
            ..Default::default()
        });
    LocalFunctionPlugin::new(
        manifest,
        Box::new(|_cap, input| {
            let prompt = input.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
            Ok(json!({ "answer": format!("LLM answer for: {prompt}") }))
        }),
    )
}

/// 安装并激活（install → validate → register → activate 全链）。
fn install_active(reg: &mut PluginRegistry, id: &str, plugin: Box<dyn Plugin>) {
    reg.install(plugin).unwrap();
    reg.validate(id).unwrap();
    reg.register(id).unwrap();
    reg.activate(id).unwrap();
}

fn youtube_plugin(id: &str, capability: &str) -> LocalFunctionPlugin {
    let manifest =
        PluginManifest::new(id, id, "1.0.0", RuntimeType::Local).with_capability(capability);
    let capability = capability.to_string();
    LocalFunctionPlugin::new(
        manifest,
        Box::new(move |_cap, _input| Ok(json!({ "ok": true, "capability": capability }))),
    )
}

// ── P1：Manifest 解析 + 序列化 roundtrip ──────────────────────────────
#[tokio::test]
async fn tc_p1_manifest_roundtrip() {
    let m = llm_plugin().manifest().clone();
    let s = serde_json::to_string(&m).unwrap();
    let back: PluginManifest = serde_json::from_str(&s).unwrap();
    assert_eq!(m.id, back.id);
    assert_eq!(m.runtime, back.runtime);
    assert_eq!(m.capabilities.len(), back.capabilities.len());
    assert!(
        back.permissions
            .network
            .contains(&"api.openai.com".to_string())
    );
    assert!(back.config[0].secret, "secret 配置标记保留");
}

// ── P2：Lifecycle（INSTALLED→VALIDATED→REGISTERED→ACTIVE→DISABLED→UNINSTALLED）──
#[tokio::test]
async fn tc_p2_plugin_lifecycle() {
    let mut reg = PluginRegistry::new();
    reg.install(Box::new(file_read_plugin())).unwrap();
    assert_eq!(
        reg.status("local.filesystem"),
        Some(PluginStatus::Installed)
    );

    // 四重校验 → 注册 → 激活
    reg.validate("local.filesystem").unwrap();
    assert_eq!(
        reg.status("local.filesystem"),
        Some(PluginStatus::Validated)
    );
    reg.register("local.filesystem").unwrap();
    reg.activate("local.filesystem").unwrap();
    assert_eq!(reg.status("local.filesystem"), Some(PluginStatus::Active));

    // 非法跳转拒绝（Active → Installed 逆流）
    assert!(reg.validate("local.filesystem").is_err());
    // 禁用/卸载
    reg.disable("local.filesystem").unwrap();
    assert_eq!(reg.status("local.filesystem"), Some(PluginStatus::Disabled));
    reg.uninstall("local.filesystem").unwrap();
    assert_eq!(
        reg.status("local.filesystem"),
        Some(PluginStatus::Uninstalled)
    );
    // 重复安装拒绝
    assert!(reg.install(Box::new(file_read_plugin())).is_err());
    reg.install(Box::new(file_read_plugin())).unwrap_err();
}

// ── P3：Capability Binding（安装 → 自动注册提供能力）───────────────────
#[tokio::test]
async fn tc_p3_capability_binding() {
    let mut reg = PluginRegistry::new();
    install_active(&mut reg, "local.filesystem", Box::new(file_read_plugin()));

    let mut caps = CapabilityRegistry::new();
    caps.declare(CapabilityContract {
        capability: "file.read".into(),
        input_schema: json!({ "type": "object" }),
        output_schema: json!({ "type": "object" }),
        effects: vec!["FileRead".into()],
    });
    let bound = caps.bind(reg.get("local.filesystem").unwrap().manifest());
    assert_eq!(bound, 1, "1 个 capability 绑定");
    let providers = caps.providers_of("file.read");
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].plugin_id, "local.filesystem");
}

// ── P4：Router（explicit / first / best_effort）───────────────────────
#[tokio::test]
async fn tc_p4_router_selection() {
    let mut caps = CapabilityRegistry::new();
    caps.declare(CapabilityContract {
        capability: "video.generate".into(),
        input_schema: json!({}),
        output_schema: json!({}),
        effects: vec!["VideoCreated".into()],
    });
    // 三个 provider：Kling / Runway / Local
    caps.bind_provider(ProviderBinding {
        capability: "video.generate".into(),
        plugin_id: "kling".into(),
        tool_id: "kling.video.generate".into(),
        cost: 10,
        latency_ms: 300,
        reliability: 0.9,
    });
    caps.bind_provider(ProviderBinding {
        capability: "video.generate".into(),
        plugin_id: "runway".into(),
        tool_id: "runway.video.generate".into(),
        cost: 5,
        latency_ms: 200,
        reliability: 0.95,
    });
    caps.bind_provider(ProviderBinding {
        capability: "video.generate".into(),
        plugin_id: "local".into(),
        tool_id: "local.video.generate".into(),
        cost: 1,
        latency_ms: 50,
        reliability: 0.7,
    });

    // First：注册序
    let r1 = Router::new(RouterStrategy::First);
    assert_eq!(
        r1.route(&caps, "video.generate")
            .map(|p| p.plugin_id.as_str()),
        Some("kling")
    );
    // Explicit
    let r2 = Router::new(RouterStrategy::Explicit("runway".into()));
    assert_eq!(
        r2.route(&caps, "video.generate")
            .map(|p| p.plugin_id.as_str()),
        Some("runway")
    );
    // BestEffort：reliability 优先（runway 0.95）
    let r3 = Router::new(RouterStrategy::BestEffort);
    assert_eq!(
        r3.route(&caps, "video.generate")
            .map(|p| p.plugin_id.as_str()),
        Some("runway"),
        "best_effort 按可靠性选"
    );
    // 未知能力 → None
    assert!(r1.route(&caps, "missing.cap").is_none());
}

// ── P5：Runtime 执行（Local Function 真实执行 + 审计）─────────────────
#[tokio::test]
async fn tc_p5_runtime_executes_and_audits() {
    let mut reg = PluginRegistry::new();
    install_active(&mut reg, "local.filesystem", Box::new(file_read_plugin()));
    let mut caps = CapabilityRegistry::new();
    caps.declare(CapabilityContract {
        capability: "file.read".into(),
        input_schema: json!({}),
        output_schema: json!({}),
        effects: vec!["FileRead".into()],
    });
    caps.bind(reg.get("local.filesystem").unwrap().manifest());

    let mut log = SessionLog::new(SessionId::new("s1"));
    let mut runtime =
        PluginRuntime::new(&reg, &caps, Router::new(RouterStrategy::First)).with_log(&mut log);
    let result = runtime
        .execute("file.read", json!({ "path": "/workspace/input/a.txt" }))
        .unwrap();
    assert_eq!(result["content"], "content of /workspace/input/a.txt");

    // 审计：capability.selected + plugin.call + plugin.result
    let tys: Vec<&str> = log.events().iter().map(|e| e.event_type.as_str()).collect();
    assert!(tys.contains(&"capability.selected"));
    assert!(tys.contains(&"plugin.call"));
    assert!(tys.contains(&"plugin.result"));
}

// ── P6：闭环 — Skill requires → Capability → Plugin → 执行 ────────────
#[tokio::test]
async fn tc_p6_skill_to_plugin_closed_loop() {
    // Skill（Phase 9 语义）：requires web.search
    let mut reg = PluginRegistry::new();
    install_active(
        &mut reg,
        "web.search.plugin",
        Box::new(youtube_plugin("web.search.plugin", "web.search")),
    );
    let mut caps = CapabilityRegistry::new();
    caps.declare(CapabilityContract {
        capability: "web.search".into(),
        input_schema: json!({}),
        output_schema: json!({}),
        effects: vec!["SearchCompleted".into()],
    });
    caps.bind(reg.get("web.search.plugin").unwrap().manifest());

    // Skill Asset 声明 requires web.search → registry 有实现
    assert!(
        !caps.providers_of("web.search").is_empty(),
        "Skill requires → 找到 Provider"
    );
    let mut runtime = PluginRuntime::new(&reg, &caps, Router::new(RouterStrategy::First));
    let out = runtime
        .execute("web.search", json!({ "q": "rust" }))
        .unwrap();
    assert_eq!(out["ok"], true);
}

// ── P7：权限（fail-closed）+ Secret 不进日志 ──────────────────────────
#[tokio::test]
async fn tc_p7_permission_and_secret_isolation() {
    // 无 network 权限的插件
    let mut reg = PluginRegistry::new();
    install_active(&mut reg, "local.filesystem", Box::new(file_read_plugin()));

    // 请求 network → DENY（fail-closed）
    let denied = PluginRuntime::check_permission(
        reg.get("local.filesystem").unwrap().manifest(),
        &PermissionRequest {
            network: Some("api.openai.com"),
            filesystem_read: None,
            filesystem_write: None,
            process: None,
            secret: None,
        },
    );
    assert!(denied.is_err(), "未声明 network → DENY");
    // 声明的 filesystem read → ALLOW
    let allowed = PluginRuntime::check_permission(
        reg.get("local.filesystem").unwrap().manifest(),
        &PermissionRequest {
            network: None,
            filesystem_read: Some("/workspace/input/x.txt"),
            filesystem_write: None,
            process: None,
            secret: None,
        },
    );
    assert!(allowed.is_ok());

    // Secret 不进日志：execute 时 input 含 secret → 日志 payload 脱敏
    let mut reg2 = PluginRegistry::new();
    install_active(&mut reg2, "ai.llm", Box::new(llm_plugin()));
    let mut caps = CapabilityRegistry::new();
    caps.declare(CapabilityContract {
        capability: "llm.generate".into(),
        input_schema: json!({}),
        output_schema: json!({}),
        effects: vec!["AnswerGenerated".into()],
    });
    caps.bind(reg2.get("ai.llm").unwrap().manifest());

    let mut log = SessionLog::new(SessionId::new("s2"));
    let mut runtime =
        PluginRuntime::new(&reg2, &caps, Router::new(RouterStrategy::First)).with_log(&mut log);
    runtime
        .execute(
            "llm.generate",
            json!({ "prompt": "hi", "api_key": "sk-secret-123" }),
        )
        .unwrap();
    let serialized = serde_json::to_string(&log).unwrap();
    assert!(
        !serialized.contains("sk-secret-123"),
        "secret 值不得进入日志（脱敏）"
    );
}

// ── P8：无实现能力 → 不可执行 ─────────────────────────────────────────
#[tokio::test]
async fn tc_p8_unresolved_capability() {
    let reg = PluginRegistry::new();
    let mut caps = CapabilityRegistry::new();
    caps.declare(CapabilityContract {
        capability: "video.generate".into(),
        input_schema: json!({}),
        output_schema: json!({}),
        effects: vec!["VideoCreated".into()],
    });
    let mut runtime = PluginRuntime::new(&reg, &caps, Router::new(RouterStrategy::First));
    let err = runtime.execute("video.generate", json!({})).unwrap_err();
    assert!(
        matches!(err, RuntimeError::Unresolved(_)),
        "无实现 → UNRESOLVED（不进入执行路径）"
    );
}

// ── P9：YouTube MVP — research/script/image/video/voice/subtitle 全链路 ─
#[tokio::test]
async fn tc_p9_youtube_full_chain() {
    let mut reg = PluginRegistry::new();
    for (id, cap) in [
        ("yt.research", "research"),
        ("yt.script", "script.generate"),
        ("yt.image", "image.generate"),
        ("yt.video", "video.generate"),
        ("yt.voice", "voice.generate"),
        ("yt.subtitle", "subtitle.generate"),
        ("yt.analytics", "analytics"),
    ] {
        install_active(&mut reg, id, Box::new(youtube_plugin(id, cap)));
    }
    let mut caps = CapabilityRegistry::new();
    for cap in [
        "research",
        "script.generate",
        "image.generate",
        "video.generate",
        "voice.generate",
        "subtitle.generate",
        "analytics",
    ] {
        caps.declare(CapabilityContract {
            capability: cap.into(),
            input_schema: json!({}),
            output_schema: json!({}),
            effects: vec![format!("{cap}.done")],
        });
    }
    for p in reg.active_plugins() {
        caps.bind(p.manifest());
    }
    // 全链路执行（每个 capability 路由到插件并成功返回）
    let mut runtime = PluginRuntime::new(&reg, &caps, Router::new(RouterStrategy::First));
    for cap in [
        "research",
        "script.generate",
        "image.generate",
        "video.generate",
        "voice.generate",
        "subtitle.generate",
        "analytics",
    ] {
        let out = runtime.execute(cap, json!({})).unwrap();
        assert_eq!(out["ok"], true, "{cap} 执行成功");
    }
}

// ── P-Rev：反向 — Plugin 不能提供未声明 Capability；Skill 不能直绑 Provider ──
#[tokio::test]
async fn tc_prev_principles_hold() {
    let mut reg = PluginRegistry::new();
    install_active(&mut reg, "local.filesystem", Box::new(file_read_plugin())); // 只声明 file.read
    let mut caps = CapabilityRegistry::new();
    caps.declare(CapabilityContract {
        capability: "file.write".into(),
        input_schema: json!({}),
        output_schema: json!({}),
        effects: vec!["FileWritten".into()],
    });
    // 绑定只来自 manifest.capabilities——file.write 不在声明 → 无 provider
    let bound = caps.bind(reg.get("local.filesystem").unwrap().manifest());
    assert_eq!(bound, 1, "只绑定声明的 file.read");
    assert!(
        caps.providers_of("file.write").is_empty(),
        "未声明能力无实现（P-Rev）"
    );

    // Skill 不能直接绑定 Provider：没有 Skill→Plugin 直连 API（只能经 capability + Router）
    let mut runtime = PluginRuntime::new(&reg, &caps, Router::new(RouterStrategy::First));
    assert!(runtime.execute("file.write", json!({})).is_err());
}

// 防未使用告警
#[allow(dead_code)]
fn _unused(_: &PluginError, _: &Value) {}
