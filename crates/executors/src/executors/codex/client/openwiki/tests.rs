//! Exercise the real launch and JSON-RPC boundary without model calls or OpenWiki writes.

use std::{
    io::{BufRead, Write},
    sync::Arc,
};

use codex_app_server_protocol::{JSONRPCRequest, RequestId};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::{AppServerClient, Duration, REQUIRED_TOOLS};
use crate::{
    env::{ExecutionEnv, RepoContext},
    executors::{
        ExecutorError,
        codex::{
            Codex,
            client::LogWriter,
            jsonrpc::{ExitSignalSender, JsonRpcPeer},
        },
    },
    profile::ExecutionMode,
};

fn emit(value: Value) {
    println!("{value}");
    std::io::stdout().flush().unwrap();
}

#[test]
#[ignore = "subprocess fixture, invoked by OpenWiki transport tests"]
fn mcp_stdio_fixture() {
    let case = std::env::var("EVK_OPENWIKI_MCP_FIXTURE").unwrap();
    let mut calls = Vec::new();
    let mut verified = false;
    let mut queries = 0;
    for line in std::io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let method = request["method"].as_str().unwrap();
        calls.push(method.to_owned());
        let result = match method {
            "account/read" => json!({"account":null,"requiresOpenaiAuth":false}),
            "thread/start" | "thread/resume" => {
                let config = &request["params"]["config"];
                if case == "reviewer" {
                    assert_eq!(config["mcp_servers.openwiki.enabled"], false);
                    assert_ne!(config["mcp_servers.openwiki.required"], true);
                    assert_eq!(request["params"]["sandbox"], "read-only");
                    assert_eq!(request["params"]["approvalPolicy"], "never");
                } else if case == "ordinary" {
                    assert!(config.get("mcp_servers.openwiki.required").is_none());
                } else {
                    assert_eq!(config["mcp_servers.openwiki.enabled"], true);
                    assert_eq!(config["mcp_servers.openwiki.required"], true);
                    assert_eq!(config["mcp_servers.openwiki.startup_timeout_sec"], 10);
                    assert_eq!(
                        config["mcp_servers.openwiki.args"],
                        json!(["mcp", "--host", "codex"])
                    );
                    assert!(config.get("mcp_optional_startup_grace_ms").is_none());
                }
                if case == "startup_error" {
                    emit(
                        json!({"id":request["id"],"error":{"code":-32603,"message":"required OpenWiki MCP startup failed"}}),
                    );
                    continue;
                }
                json!({
                    "thread": {
                        "id":"writer-thread", "sessionId":"writer-session", "preview":"",
                        "ephemeral":false, "modelProvider":"openai", "createdAt":0,
                        "updatedAt":0, "status":{"type":"idle"}, "cwd":"/tmp",
                        "cliVersion":"fixture", "source":"cli", "turns":[]
                    },
                    "model":"gpt-6-astra", "modelProvider":"openai", "cwd":"/tmp",
                    "approvalPolicy":"never", "approvalsReviewer":"user",
                    "sandbox":{"type":"readOnly"}, "reasoningEffort":"max"
                })
            }
            "mcpServerStatus/list" => {
                queries += 1;
                assert_eq!(request["params"]["threadId"], "writer-thread");
                assert_eq!(request["params"]["detail"], "toolsAndAuthOnly");
                assert!(!matches!(case.as_str(), "ordinary" | "reviewer"));
                if case == "hang" {
                    continue;
                }
                if case == "rpc_error" {
                    emit(
                        json!({"id":request["id"],"error":{"code":-32603,"message":"catalog failed"}}),
                    );
                    continue;
                }
                if case == "slow" {
                    // Longer than Codex's optional startup grace. The first model
                    // request must remain behind this real RPC acknowledgement.
                    std::thread::sleep(Duration::from_millis(1200));
                }
                if case == "absent" {
                    json!({"data":[],"nextCursor":null})
                } else if case == "cursor_loop" {
                    json!({"data":[],"nextCursor":"same"})
                } else if case == "too_many_pages" {
                    json!({"data":[],"nextCursor":queries.to_string()})
                } else if case == "paginated" && queries == 1 {
                    assert!(request["params"]["cursor"].is_null());
                    json!({"data":[],"nextCursor":"second-page"})
                } else {
                    if case == "paginated" {
                        assert_eq!(request["params"]["cursor"], "second-page");
                    }
                    if case == "ready_without_tools" {
                        emit(json!({"method":"mcpServer/startupStatus/updated","params":{
                            "threadId":"writer-thread","name":"openwiki","status":"ready","error":null
                        }}));
                    }
                    let tools: serde_json::Map<String, Value> = REQUIRED_TOOLS.iter()
                        .filter(|&&name| case != "missing_finish" || name != "openwiki_finish")
                        .filter(|_| case != "ready_without_tools")
                        .map(|name| ((*name).into(), json!({
                            "name": if case == "wrong_tool_name" { "different_tool" } else { name },
                            "inputSchema":{"type":"object"}
                        }))).collect();
                    verified = matches!(case.as_str(), "ready" | "slow" | "paginated");
                    json!({"data":[{
                        "name":if case == "other_server" {"unrelated"} else {"openwiki"},
                        "tools":tools,"resources":[],"resourceTemplates":[],"authStatus":"unsupported"
                    }],"nextCursor":null})
                }
            }
            "thread/settings/update" | "thread/goal/clear" => {
                assert!(verified);
                json!({})
            }
            "turn/start" => {
                assert!(verified || matches!(case.as_str(), "ordinary" | "reviewer"));
                json!({"turn":{"id":"turn-1","items":[],"status":"inProgress","error":null}})
            }
            "thread/goal/set" => {
                assert!(verified);
                json!({"goal":{
                    "threadId":"writer-thread", "objective":"fixture", "status":"active",
                    "tokenBudget":null,"tokensUsed":0,"timeUsedSeconds":0,"createdAt":1,"updatedAt":1
                }})
            }
            "fixture/state" => json!({"calls":calls,"queries":queries,"verified":verified}),
            other => panic!("Unexpected fixture request: {other}"),
        };
        emit(json!({"id":request["id"],"result":result}));
    }
}

struct Harness {
    client: Arc<AppServerClient>,
    peer: JsonRpcPeer,
    child: tokio::process::Child,
    cancel: CancellationToken,
}

impl Harness {
    async fn new(case: &str, mode: ExecutionMode) -> Self {
        let cancel = CancellationToken::new();
        let client = AppServerClient::new(
            LogWriter::new(tokio::io::sink()),
            None,
            false,
            matches!(mode, ExecutionMode::Plan | ExecutionMode::PlanWithGoal),
            mode,
            None,
            None,
            RepoContext::default(),
            false,
            String::new(),
            cancel.clone(),
        );
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "executors::codex::client::openwiki::tests::mcp_stdio_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("EVK_OPENWIKI_MCP_FIXTURE", case)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let (exit, _exit_rx) = tokio::sync::oneshot::channel();
        let peer = JsonRpcPeer::spawn(
            child.stdin.take().unwrap(),
            child.stdout.take().unwrap(),
            client.clone(),
            ExitSignalSender::new(exit),
            cancel.clone(),
        );
        client.connect(peer.clone());
        Self {
            client,
            peer,
            child,
            cancel,
        }
    }

    async fn launch(&self, case: &str, resume: bool) -> Result<(), ExecutorError> {
        let codex: Codex = serde_json::from_value(json!({"sandbox":"danger-full-access"})).unwrap();
        let mut env = ExecutionEnv::new(RepoContext::default(), false, String::new());
        if case != "ordinary" {
            env.insert("EVK_OPENWIKI_MAINTENANCE", "1");
        }
        if case == "reviewer" {
            env.insert("EVK_OPENWIKI_REVIEWER", "1");
        }
        let params =
            codex.build_thread_start_params_with_resources(std::path::Path::new("/tmp"), &env);
        tokio::time::timeout(
            Duration::from_secs(5),
            Codex::launch_codex_agent(
                params,
                resume.then(|| "writer-thread".into()),
                "fixture".into(),
                vec![],
                self.client.clone(),
            ),
        )
        .await
        .expect("launch bounded by test")
    }

    async fn state(&self) -> Value {
        self.peer
            .request(
                RequestId::String("fixture-state".into()),
                &JSONRPCRequest {
                    id: RequestId::String("fixture-state".into()),
                    method: "fixture/state".into(),
                    params: None,
                    trace: None,
                },
                "fixture/state",
                self.cancel.clone(),
            )
            .await
            .unwrap()
    }

    async fn close(mut self) {
        self.cancel.cancel();
        self.child.start_kill().ok();
        self.child.wait().await.unwrap();
    }
}

#[tokio::test]
async fn writer_tools_are_verified_before_initial_followup_and_goal_launches() {
    for mode in [
        ExecutionMode::Code,
        ExecutionMode::Plan,
        ExecutionMode::PlanWithGoal,
        ExecutionMode::Goal,
    ] {
        for resume in [false, true] {
            let harness = Harness::new("paginated", mode).await;
            harness.launch("paginated", resume).await.unwrap();
            let state = harness.state().await;
            assert_eq!(state["queries"], 2);
            let calls: Vec<String> = serde_json::from_value(state["calls"].clone()).unwrap();
            let check = calls
                .iter()
                .rposition(|c| c == "mcpServerStatus/list")
                .unwrap();
            let start = calls
                .iter()
                .position(|c| matches!(c.as_str(), "turn/start" | "thread/goal/set"))
                .unwrap();
            assert!(check < start);
            harness.close().await;
        }
    }
}

#[tokio::test]
async fn slow_tool_discovery_does_not_start_a_writer_early() {
    let harness = Harness::new("slow", ExecutionMode::Code).await;
    let started = std::time::Instant::now();
    harness.launch("slow", false).await.unwrap();
    assert!(started.elapsed() >= Duration::from_millis(1200));
    assert_eq!(harness.state().await["verified"], true);
    harness.close().await;
}

#[tokio::test]
async fn unavailable_or_incomplete_mcp_never_activates_a_writer() {
    for case in [
        "startup_error",
        "rpc_error",
        "absent",
        "ready_without_tools",
        "missing_finish",
        "other_server",
        "wrong_tool_name",
        "cursor_loop",
        "too_many_pages",
    ] {
        for mode in [ExecutionMode::Code, ExecutionMode::Goal] {
            let harness = Harness::new(case, mode).await;
            let error = harness.launch(case, false).await.unwrap_err().to_string();
            assert!(
                error.contains(if case == "startup_error" {
                    "startup failed"
                } else {
                    "OpenWiki MCP preflight"
                }),
                "{error}"
            );
            if case == "missing_finish" {
                assert!(error.contains("openwiki_finish"));
            }
            let calls: Vec<String> =
                serde_json::from_value(harness.state().await["calls"].clone()).unwrap();
            assert!(!calls.iter().any(|c| matches!(
                c.as_str(),
                "turn/start" | "thread/goal/set" | "thread/goal/clear" | "thread/settings/update"
            )));
            harness.close().await;
        }
    }
}

#[tokio::test]
async fn reviewer_and_ordinary_chat_do_not_require_or_enable_writer_tools() {
    for case in ["reviewer", "ordinary"] {
        for resume in [false, true] {
            let harness = Harness::new(case, ExecutionMode::Code).await;
            harness.launch(case, resume).await.unwrap();
            assert_eq!(harness.state().await["queries"], 0);
            harness.close().await;
        }
    }
}

#[tokio::test]
async fn preflight_timeout_and_cancellation_are_bounded() {
    for cancelled in [false, true] {
        let harness = Harness::new("hang", ExecutionMode::Code).await;
        harness
            .client
            .register_session("writer-thread")
            .await
            .unwrap();
        let cancel = harness.cancel.clone();
        let interrupter = tokio::spawn(async move {
            if cancelled {
                tokio::time::sleep(Duration::from_millis(30)).await;
                cancel.cancel();
            }
        });
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            harness
                .client
                .ensure_openwiki_tools_with_timeout(Duration::from_millis(100)),
        )
        .await
        .unwrap();
        let error = result.unwrap_err();
        if !cancelled {
            assert!(
                matches!(error, ExecutorError::Io(ref e) if e.kind() == std::io::ErrorKind::TimedOut)
            );
        }
        assert!(error.to_string().contains("OpenWiki MCP preflight"));
        interrupter.await.unwrap();
        harness.close().await;
    }
}

#[tokio::test]
async fn preflight_cannot_fall_back_to_global_mcp_inventory() {
    let harness = Harness::new("ready", ExecutionMode::Code).await;
    let error = harness.client.ensure_openwiki_tools().await.unwrap_err();
    assert!(error.to_string().contains("no registered Codex thread"));
    assert_eq!(harness.state().await["queries"], 0);
    harness.close().await;
}

#[tokio::test]
#[ignore = "requires installed Codex and OpenWiki; discovery only, no model or Wiki calls"]
async fn installed_openwiki_mcp_preflight() {
    let temp = tempfile::tempdir().unwrap();
    let codex: Codex = serde_json::from_value(json!({"sandbox":"read-only"})).unwrap();
    let mut env = ExecutionEnv::new(RepoContext::default(), false, String::new());
    env.insert("EVK_OPENWIKI_MAINTENANCE", "1");
    let mut params = codex.build_thread_start_params_with_resources(temp.path(), &env);
    params.ephemeral = Some(true);
    codex
        .with_discovery_app_server(temp.path(), move |client| async move {
            // No turn/start, goal activation, or MCP tool call: only handshake and
            // inventory. The existing discovery helper cleans up the process group.
            tokio::time::timeout(Duration::from_secs(30), async {
                let response = client.thread_start(params).await?;
                client.register_session(&response.thread.id).await?;
                client.ensure_openwiki_tools().await
            })
            .await
            .map_err(|_| {
                ExecutorError::Io(std::io::Error::other("installed MCP discovery timed out"))
            })?
        })
        .await
        .unwrap();
    assert!(!temp.path().join("openwiki").exists());
}
