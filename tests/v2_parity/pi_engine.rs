//! M1b · G3 契约测试：Pi 真实引擎迁移（PiEngine: CognitiveEngine）。
//!
//! 契约：
//! - decide 经 pi 子进程（--no-tools 沙箱）产出 V2 Decision（capability/args/confidence/rule=None）。
//! - pi 输出 capability=null → Ok(None)（无更多动作，D5 完成判定）。
//! - pi 输出视图外能力 → Err(UnknownCapability)（fail-safe，不信任引擎）。
//! - pi 缺失/超时 → Err(Engine(...))；超时必须 kill+wait 回收子进程。
//! - health() 探测 pi 二进制（WB2004 前置检查同源）。
//!
//! 环境变量 PI_BIN 是进程全局，测试内以互斥锁串行化（edition 2024 set_var 为 unsafe）。

use openaip_os::cognitive::engine::{AgentView, CognitiveEngine};
use openaip_os::cognitive::pi::PiEngine;
use serde_json::json;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static PI_ENV_LOCK: Mutex<()> = Mutex::new(());

const OK: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/v2_parity/fixtures/fake_pi_ok.sh"
);
const NONE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/v2_parity/fixtures/fake_pi_none.sh"
);
const BADCAP: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/v2_parity/fixtures/fake_pi_badcap.sh"
);
const SLOW: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/v2_parity/fixtures/fake_pi_slow.sh"
);

fn set_pi_bin(path: &str) {
    unsafe { std::env::set_var("PI_BIN", path) };
}

fn clear_pi_bin() {
    unsafe { std::env::remove_var("PI_BIN") };
}

fn refund_view() -> AgentView {
    AgentView {
        goal: "退款 u1".into(),
        world: json!({}),
        related: vec![],
        memory: vec![],
        capabilities: vec!["customer.refund".into()],
        entities: vec![],
        subgraph: None,
        evolution: vec![],
    }
}

// ── G3a：结构化决策解析 ────────────────────────────────────────────────
#[tokio::test]
async fn g3a_decide_parses_structured_decision() {
    let _g = PI_ENV_LOCK.lock().unwrap();
    set_pi_bin(OK);
    let engine = PiEngine::new("test-agent");
    let d = engine
        .decide(&refund_view())
        .expect("应解析成功")
        .expect("应产出决策");
    assert_eq!(d.capability, "customer.refund");
    assert_eq!(d.args, json!({ "target": "u1", "amount": 100 }));
    assert!((d.confidence - 0.9).abs() < 1e-9);
    assert_eq!(d.rule, None);
    clear_pi_bin();
}

// ── G3b：capability=null → None（D5 完成判定路径）──────────────────────
#[tokio::test]
async fn g3b_none_when_pi_says_none() {
    let _g = PI_ENV_LOCK.lock().unwrap();
    set_pi_bin(NONE);
    let engine = PiEngine::new("test-agent");
    let d = engine.decide(&refund_view()).expect("解析");
    assert!(d.is_none(), "capability=null 应映射 None: {d:?}");
    clear_pi_bin();
}

// ── G3c：视图外能力 → UnknownCapability（fail-safe）───────────────────
#[tokio::test]
async fn g3c_unknown_capability_is_rejected() {
    let _g = PI_ENV_LOCK.lock().unwrap();
    set_pi_bin(BADCAP);
    let engine = PiEngine::new("test-agent");
    let res = engine.decide(&refund_view());
    assert!(
        matches!(
            res,
            Err(openaip_os::cognitive::CognitiveError::UnknownCapability(_))
        ),
        "视图外能力应被拒绝: {res:?}"
    );
    clear_pi_bin();
}

// ── G3d：pi 不可用 → Engine 错误（WB2004 前置检查同源）────────────────
#[tokio::test]
async fn g3d_unavailable_when_pi_missing() {
    let _g = PI_ENV_LOCK.lock().unwrap();
    set_pi_bin(concat!(env!("CARGO_MANIFEST_DIR"), "/definitely/not/pi"));
    let engine = PiEngine::new("test-agent");
    let res = engine.decide(&refund_view());
    assert!(
        matches!(res, Err(openaip_os::cognitive::CognitiveError::Engine(_))),
        "pi 缺失应 Engine 错误: {res:?}"
    );
    clear_pi_bin();
}

// ── G3e：超时杀进程（不遗留孤儿）──────────────────────────────────────
#[tokio::test]
async fn g3e_timeout_kills_child() {
    let _g = PI_ENV_LOCK.lock().unwrap();
    set_pi_bin(SLOW);
    let engine = PiEngine::new("test-agent").with_timeout(Duration::from_millis(1000));
    let start = Instant::now();
    let res = engine.decide(&refund_view());
    let elapsed = start.elapsed();
    clear_pi_bin();
    assert!(
        matches!(res, Err(openaip_os::cognitive::CognitiveError::Engine(ref m)) if m.contains("超时")),
        "应超时错误: {res:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "超时应快速返回: {elapsed:?}"
    );
}

// ── G3f：health 探测（PI_BIN 优先，PATH 兜底）──────────────────────────
#[tokio::test]
async fn g3f_health_probes_binary() {
    let _g = PI_ENV_LOCK.lock().unwrap();
    set_pi_bin(OK);
    assert!(PiEngine::new("t").health(), "PI_BIN 存在 → 可用");
    set_pi_bin(concat!(env!("CARGO_MANIFEST_DIR"), "/definitely/not/pi"));
    assert!(!PiEngine::new("t").health(), "PI_BIN 指向不存在 → 不可用");
    clear_pi_bin();
}
