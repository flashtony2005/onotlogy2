//! DSH 适配器契约：命令覆盖与 agent_task 真实执行链。

mod common;

use openaip_os::storage::{LibSQLStorage, TraceQuery};
use openaip_os::worker::adapters::{dsh_worker, shell_worker};
use openaip_os::worker::sdk::{Worker, WorkerError};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;

struct DshCommandGuard(Option<std::ffi::OsString>);

impl Drop for DshCommandGuard {
    fn drop(&mut self) {
        unsafe {
            match self.0.take() {
                Some(value) => std::env::set_var("OAIP_DSH_COMMAND", value),
                None => std::env::remove_var("OAIP_DSH_COMMAND"),
            }
        }
    }
}

async fn spawn_server() -> (std::net::SocketAddr, Arc<LibSQLStorage>) {
    let storage = Arc::new(LibSQLStorage::open(":memory:").await.unwrap());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let storage_for_server = storage.clone();
    tokio::spawn(async move {
        let _ = openaip_os::server::serve(
            storage_for_server,
            common::retain_services(),
            common::retain_defs(),
            Arc::new(common::v2_retain_engine()),
            listener,
            "ui/dist",
            false,
            false,
            false,
        )
        .await;
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    (addr, storage)
}

#[tokio::test(flavor = "multi_thread")]
async fn tc_dsh_env_override_routes_agent_task_to_real_process() {
    // /bin/echo 是确定性的真实进程：既验证 OAIP_DSH_COMMAND 注入，又避免依赖本机 dsh/LLM。
    let _env_guard = DshCommandGuard(std::env::var_os("OAIP_DSH_COMMAND"));
    unsafe {
        std::env::set_var("OAIP_DSH_COMMAND", "/bin/echo");
    }

    async {
        let (addr, storage) = spawn_server().await;
        let workspace = std::env::temp_dir();
        let body = serde_json::json!({
            "sessionId": "sess-dsh-env",
            "sender": "oaip-test",
            "intent": "agent_task",
            "executionContext": {
                "workspace": { "root": workspace, "mode": "isolated" }
            },
            "payload": { "task": "dsh-env-contract" }
        })
        .to_string();
        let (_, body) =
            common::http_call(addr, "POST", "/api/v1/facade/fabric/submit", Some(&body)).await;
        let submission: Value = serde_json::from_str(&body).unwrap();
        let result = &submission["data"]["result"];
        assert_eq!(
            result["receiver"], "dsh",
            "agent_task 应路由到 dsh: {submission}"
        );
        assert_eq!(result["payload"]["status"], "running");
        let run_id = result["correlation_id"].as_str().unwrap();

        for _ in 0..100 {
            let events = storage
                .list_events(TraceQuery {
                    event_type: Some("fabric.task_completed".into()),
                    ..Default::default()
                })
                .await
                .unwrap();
            if let Some(event) = events.iter().find(|event| {
                event
                    .input
                    .as_ref()
                    .and_then(|input| input["correlation_id"].as_str())
                    == Some(run_id)
            }) {
                let input = event.input.as_ref().unwrap();
                assert_eq!(input["to"], "completed");
                assert_eq!(input["agent"], "dsh");
                assert!(input["duration_ms"].is_number());
                assert_eq!(input["exit_code"], 0);
                assert_eq!(input["progress_count"], 1);
                assert_eq!(input["structured_output"]["format"], "text");
                assert_eq!(input["failure_category"], Value::Null);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let events = storage
            .list_events(TraceQuery {
                event_type: Some("fabric.task_completed".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            events.iter().any(|event| {
                event
                    .input
                    .as_ref()
                    .and_then(|input| input["correlation_id"].as_str())
                    == Some(run_id)
            }),
            "agent_task 未产生 fabric.task_completed"
        );
    }
    .await
}

#[tokio::test]
async fn dsh_and_shell_workers_require_isolated_context() {
    let dsh = dsh_worker(Some("/bin/echo"));
    let shell = shell_worker(Some("/bin/echo"));

    for worker in [dsh, shell] {
        let error = worker
            .execute(
                "agent.task",
                serde_json::json!({ "args": ["must-not-run"] }),
            )
            .await
            .expect_err("sandboxed providers must reject a missing context");
        assert!(
            matches!(error, WorkerError::Execution(ref message) if message.contains("workspace")),
            "unexpected error: {error:?}"
        );
    }
}

#[tokio::test]
async fn dsh_and_shell_workers_reject_unallowlisted_environment() {
    let root = std::env::temp_dir();
    for (worker, capability) in [
        (dsh_worker(Some("/bin/echo")), "agent.task"),
        (shell_worker(Some("/bin/echo")), "shell.execute"),
    ] {
        let error = worker
            .execute(
                capability,
                serde_json::json!({
                    "execution_context": {
                        "workspace": { "root": root, "mode": "isolated" },
                        "env": { "OAIP_UNTRUSTED_SECRET": "must-not-cross" }
                    },
                    "args": ["must-not-run"]
                }),
            )
            .await
            .expect_err("provider environment policy must fail closed");
        assert!(
            matches!(error, WorkerError::Execution(ref message) if message.contains("OAIP_UNTRUSTED_SECRET")),
            "unexpected error: {error:?}"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn process_timeout_kills_the_entire_process_group() {
    let root = std::env::temp_dir();
    let marker = root.join(format!(
        "oaip_timeout_marker_{}_{}",
        std::process::id(),
        openaip_os::trace::new_trace_id()
    ));
    let _ = std::fs::remove_file(&marker);

    let worker = shell_worker(Some("/bin/sh"));
    let error = worker
        .execute(
            "shell.execute",
            serde_json::json!({
                "execution_context": {
                    "workspace": { "root": root, "mode": "isolated" },
                    "timeout_ms": 40
                },
                "args": ["-c", format!("sleep 1; touch {}", marker.display())]
            }),
        )
        .await
        .expect_err("timeout must terminate the process");
    assert!(matches!(error, WorkerError::Execution(message) if message.contains("超时")));

    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        !marker.exists(),
        "a timed-out child must not continue after the parent is killed"
    );
    let _ = std::fs::remove_file(marker);
}
