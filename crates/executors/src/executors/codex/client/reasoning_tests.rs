//! Exercise the real JSON-RPC boundary without authentication or model calls.

use std::io::{BufRead, Write};

use codex_app_server_protocol::{ThreadResumeParams, ThreadStartParams};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::{AppServerClient, LogWriter, ProtocolReasoningEffort};
use crate::{
    executors::codex::jsonrpc::{ExitSignalSender, JsonRpcPeer},
    profile::ExecutionMode,
};

#[test]
#[ignore = "subprocess fixture, invoked by reasoning transport tests"]
fn reasoning_stdio_fixture() {
    let inherited: Value =
        serde_json::from_str(&std::env::var("EVK_REASONING_FIXTURE").unwrap()).unwrap();
    let mut resolved = Value::Null;
    for line in std::io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let result = match request["method"].as_str().unwrap() {
            "thread/start" | "thread/resume" => {
                resolved = request["params"]["config"]
                    .get("model_reasoning_effort")
                    .cloned()
                    .unwrap_or_else(|| inherited.clone());
                let mut thread = json!({
                    "id": "reasoning-thread", "sessionId": "reasoning-session",
                    "preview": "", "ephemeral": false, "modelProvider": "openai",
                    "createdAt": 0, "updatedAt": 0, "status": {"type": "idle"},
                    "cwd": "/tmp", "cliVersion": "fixture", "source": "cli", "turns": []
                });
                if request["method"] == "thread/resume" {
                    // Resume must still tolerate new display-only history variants.
                    thread["turns"] = json!([{"items": [{"type": "futureHistoryItem"}]}]);
                }
                json!({
                    "thread": thread, "model": "gpt-6-astra", "modelProvider": "openai",
                    "cwd": "/tmp", "approvalPolicy": "never", "approvalsReviewer": "user",
                    "sandbox": {"type": "readOnly"}, "reasoningEffort": resolved
                })
            }
            "turn/start" => {
                assert_eq!(request["params"]["effort"], resolved);
                assert_eq!(
                    request["params"]["collaborationMode"]["settings"]["reasoning_effort"],
                    resolved
                );
                json!({"turn": {"id": "turn-1", "items": [], "status": "inProgress", "error": null}})
            }
            "thread/settings/update" => {
                resolved =
                    request["params"]["collaborationMode"]["settings"]["reasoning_effort"].clone();
                assert_eq!(request["params"]["effort"], resolved);
                json!({})
            }
            other => panic!("Unexpected fixture request: {other}"),
        };
        println!("{}", json!({"id": request["id"], "result": result}));
        std::io::stdout().flush().unwrap();
    }
}

#[tokio::test]
async fn resolved_reasoning_survives_start_resume_and_mode_selection() {
    for mode in [
        ExecutionMode::Code,
        ExecutionMode::Plan,
        ExecutionMode::PlanWithGoal,
        ExecutionMode::Goal,
    ] {
        for resume in [false, true] {
            for inherited in ["null", "\"xhigh\"", "\"max\"", "\"ultra\""] {
                for explicit in [None, Some(ProtocolReasoningEffort::High)] {
                    let cancel = CancellationToken::new();
                    let client = AppServerClient::new(
                        LogWriter::new(tokio::io::sink()),
                        None,
                        false,
                        matches!(mode, ExecutionMode::Plan | ExecutionMode::PlanWithGoal),
                        mode,
                        None,
                        explicit.clone(),
                        Default::default(),
                        false,
                        String::new(),
                        cancel.clone(),
                    );
                    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
                        .args([
                            "--exact",
                            "executors::codex::client::reasoning_tests::reasoning_stdio_fixture",
                            "--ignored",
                            "--nocapture",
                        ])
                        .env("EVK_REASONING_FIXTURE", inherited)
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::inherit())
                        .kill_on_drop(true)
                        .spawn()
                        .unwrap();
                    let (exit_tx, _exit_rx) = tokio::sync::oneshot::channel();
                    let peer = JsonRpcPeer::spawn(
                        child.stdin.take().unwrap(),
                        child.stdout.take().unwrap(),
                        client.clone(),
                        ExitSignalSender::new(exit_tx),
                        cancel.clone(),
                    );
                    client.connect(peer);
                    let config = explicit.as_ref().map(|effort| {
                        [("model_reasoning_effort".into(), json!(effort))]
                            .into_iter()
                            .collect()
                    });
                    let exercise = async {
                        if resume {
                            client
                                .thread_resume(ThreadResumeParams {
                                    thread_id: "reasoning-thread".into(),
                                    config,
                                    ..Default::default()
                                })
                                .await
                                .unwrap();
                        } else {
                            client
                                .thread_start(ThreadStartParams {
                                    config,
                                    ..Default::default()
                                })
                                .await
                                .unwrap();
                        }
                        let expected = explicit.or(serde_json::from_str(inherited).unwrap());
                        assert_eq!(client.current_reasoning_effort(), expected);
                        let selected = client.initial_collaboration_mode().unwrap();
                        assert_eq!(selected.settings.reasoning_effort, expected);
                        // Plan -> Code and post-approval turns reuse this state too.
                        let code = client
                            .collaboration_mode(codex_protocol::config_types::ModeKind::Default)
                            .unwrap();
                        assert_eq!(code.settings.reasoning_effort, expected);
                        client
                            .turn_start_with_mode("reasoning-thread".into(), vec![], Some(selected))
                            .await
                            .unwrap();
                        for changed in [Some(ProtocolReasoningEffort::High), None] {
                            let params = client
                                .build_reasoning_thread_settings_update_params(
                                    "reasoning-thread".into(),
                                    changed.clone(),
                                )
                                .unwrap();
                            client.thread_settings_update(params).await.unwrap();
                            assert_eq!(client.current_reasoning_effort(), changed);
                            client
                                .turn_start_with_mode(
                                    "reasoning-thread".into(),
                                    vec![],
                                    Some(client.initial_collaboration_mode().unwrap()),
                                )
                                .await
                                .unwrap();
                        }
                    };
                    tokio::time::timeout(std::time::Duration::from_secs(5), exercise)
                        .await
                        .unwrap();
                    cancel.cancel();
                    child.kill().await.unwrap();
                    child.wait().await.unwrap();
                }
            }
        }
    }
}

#[test]
fn resolved_default_replaces_stale_explicit_effort() {
    let client = AppServerClient::new(
        LogWriter::new(tokio::io::sink()),
        None,
        false,
        false,
        ExecutionMode::Code,
        None,
        Some(ProtocolReasoningEffort::XHigh),
        Default::default(),
        false,
        String::new(),
        CancellationToken::new(),
    );
    client.adopt_thread_settings("gpt-6-astra".into(), None);
    assert_eq!(client.current_reasoning_effort(), None);
    assert_eq!(
        client
            .initial_collaboration_mode()
            .unwrap()
            .settings
            .reasoning_effort,
        None
    );
}
