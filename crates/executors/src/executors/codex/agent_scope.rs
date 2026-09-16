//! Scope raw multiplexed app-server frames before canonical projection.
//! Audit retains the original frames, including child deltas. Only completed
//! child observations become durable activity, never parent output or control.
use std::collections::HashMap;

use super::super::provider_adapter::{DecodedProviderEvent, TypedProviderEvent};
use crate::runtime::AgentActivity;

#[derive(Debug, Default)]
pub struct AgentScope {
    root: Option<String>,
    agents: HashMap<String, (Option<String>, Option<String>)>,
}

impl AgentScope {
    pub fn apply(&mut self, event: &mut DecodedProviderEvent) {
        let Some(raw) = event.raw.payload_json.as_ref() else {
            return;
        };
        if self.root.is_none() {
            self.root = raw
                .pointer("/result/thread/id")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            if let Some(root) = &self.root {
                // The control response establishes the primary identity even
                // if thread/started arrived before it. Never adopt a child
                // notification as the Run's resumable provider session.
                event.typed = TypedProviderEvent::SessionObserved(root.clone());
            }
        }
        let Some(method) = raw.get("method").and_then(|v| v.as_str()) else {
            return;
        };
        let Some(params) = raw.get("params") else {
            return;
        };
        if matches!(method, "thread/started" | "thread.started") {
            event.typed = TypedProviderEvent::AuditOnly {
                event_type: method.to_owned(),
            };
            return;
        }
        let thread = params.get("threadId").and_then(|v| v.as_str());
        let item = params.get("item");

        if method == "item/started"
            && item.and_then(|v| v.get("type")).and_then(|v| v.as_str()) == Some("subAgentActivity")
        {
            event.typed = TypedProviderEvent::AuditOnly {
                event_type: method.to_owned(),
            };
            return;
        }

        if method == "item/completed"
            && item.and_then(|v| v.get("type")).and_then(|v| v.as_str()) == Some("subAgentActivity")
        {
            let item = item.unwrap();
            if let Some(id) = item.get("agentThreadId").and_then(|v| v.as_str()) {
                let kind = item
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let record = self.agents.entry(id.to_owned()).or_default();
                if let Some(path) = item.get("agentPath").and_then(|v| v.as_str()) {
                    record.1 = Some(path.to_owned());
                }
                if kind == "started" && thread.is_some_and(|parent| parent != id) {
                    record.0 = thread.map(str::to_owned);
                }
                // Interactions with the primary agent are not child creation.
                if self.root.as_deref() != Some(id) {
                    event.typed = TypedProviderEvent::AgentActivity(AgentActivity {
                        thread_id: id.to_owned(),
                        parent_thread_id: record.0.clone(),
                        agent_path: record.1.clone(),
                        kind: kind.to_owned(),
                        content: None,
                    });
                    return;
                }
                event.typed = TypedProviderEvent::AuditOnly {
                    event_type: method.to_owned(),
                };
                return;
            }
        }

        let Some(thread) = thread else {
            if method.starts_with("item/")
                || method.starts_with("turn/")
                || method.starts_with("thread/goal/")
            {
                event.typed = TypedProviderEvent::AuditOnly {
                    event_type: method.to_owned(),
                };
            }
            return;
        };
        if self.root.as_deref() == Some(thread) {
            return;
        }
        let (parent, path) = self.agents.get(thread).cloned().unwrap_or_default();
        let observation = match (&event.typed, method) {
            (
                TypedProviderEvent::Message {
                    content,
                    final_output,
                    ..
                },
                _,
            ) => Some((
                if *final_output {
                    "answer"
                } else {
                    "commentary"
                },
                Some(content.clone()),
            )),
            (
                TypedProviderEvent::ToolCall {
                    name,
                    arguments,
                    result,
                    ..
                },
                "item/completed",
            ) => Some((
                "tool",
                Some(format!(
                    "{name}\nArguments: {}\nResult: {}",
                    arguments.as_ref().unwrap_or(&serde_json::Value::Null),
                    result.as_ref().unwrap_or(&serde_json::Value::Null)
                )),
            )),
            (_, "turn/started") => Some(("running", None)),
            (_, "turn/completed") => Some((
                match params.pointer("/turn/status").and_then(|v| v.as_str()) {
                    Some("completed") => "completed",
                    Some("failed") => "failed",
                    Some("interrupted") => "interrupted",
                    _ => "unknown",
                },
                None,
            )),
            _ => None,
        };
        event.typed = match observation {
            Some((kind, content)) => TypedProviderEvent::AgentActivity(AgentActivity {
                thread_id: thread.to_owned(),
                parent_thread_id: parent,
                agent_path: path,
                kind: kind.to_owned(),
                content,
            }),
            None => TypedProviderEvent::AuditOnly {
                event_type: method.to_owned(),
            },
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        executors::provider_adapter::DirectProvider,
        runtime::{NativeAuditChannel, NativeAuditDirection, NativeAuditFrame},
    };

    fn event(value: serde_json::Value, sequence: u64) -> DecodedProviderEvent {
        let frame = NativeAuditFrame::from_bytes(
            sequence,
            chrono::Utc::now(),
            NativeAuditDirection::Output,
            NativeAuditChannel::Stdout,
            "application/json",
            uuid::Uuid::nil(),
            &serde_json::to_vec(&value).unwrap(),
        );
        DirectProvider::Codex.decode_native_frame(&frame).unwrap()
    }

    #[test]
    fn children_and_unknown_threads_never_become_parent_output() {
        let mut scope = AgentScope::default();
        scope.apply(&mut event(
            serde_json::json!({"result":{"thread":{"id":"root"}}}),
            1,
        ));
        let mut spawn = event(
            serde_json::json!({"method":"item/completed","params":{"threadId":"root","item":{"type":"subAgentActivity","id":"spawn","kind":"started","agentThreadId":"child","agentPath":"/root/research"}}}),
            2,
        );
        scope.apply(&mut spawn);
        assert!(
            matches!(spawn.typed, TypedProviderEvent::AgentActivity(ref a) if a.parent_thread_id.as_deref()==Some("root"))
        );
        for thread in ["child", "unknown"] {
            let mut child = event(
                serde_json::json!({"method":"item/completed","params":{"threadId":thread,"item":{"type":"agentMessage","id":"answer","text":"child answer","phase":"final_answer"}}}),
                3,
            );
            scope.apply(&mut child);
            assert!(
                matches!(child.typed, TypedProviderEvent::AgentActivity(ref a) if a.kind=="answer")
            );
            let mut delta = event(
                serde_json::json!({"method":"item/agentMessage/delta","params":{"threadId":thread,"itemId":"answer","delta":"abc"}}),
                4,
            );
            scope.apply(&mut delta);
            assert!(matches!(delta.typed, TypedProviderEvent::AuditOnly { .. }));
        }
        let mut parent = event(
            serde_json::json!({"method":"item/completed","params":{"threadId":"root","item":{"type":"agentMessage","id":"parent-answer","text":"parent answer"}}}),
            5,
        );
        scope.apply(&mut parent);
        assert!(matches!(
            parent.typed,
            TypedProviderEvent::Message {
                final_output: true,
                ..
            }
        ));
    }

    #[test]
    fn nested_agents_keep_immediate_parent_and_child_goal_is_not_parent_control() {
        let mut scope = AgentScope::default();
        scope.apply(&mut event(
            serde_json::json!({"result":{"thread":{"id":"root"}}}),
            1,
        ));
        let mut spawn = event(
            serde_json::json!({"method":"item/completed","params":{"threadId":"child","item":{"type":"subAgentActivity","id":"spawn","kind":"started","agentThreadId":"grandchild","agentPath":"/root/research/review"}}}),
            2,
        );
        scope.apply(&mut spawn);
        assert!(
            matches!(spawn.typed, TypedProviderEvent::AgentActivity(ref a) if a.parent_thread_id.as_deref()==Some("child"))
        );
        let mut goal = event(
            serde_json::json!({"method":"thread/goal/cleared","params":{"threadId":"grandchild"}}),
            3,
        );
        scope.apply(&mut goal);
        assert!(matches!(goal.typed, TypedProviderEvent::AuditOnly { .. }));
        let mut anonymous = event(
            serde_json::json!({"method":"item/completed","params":{"item":{"type":"agentMessage","id":"m","text":"unknown origin"}}}),
            4,
        );
        scope.apply(&mut anonymous);
        assert!(matches!(
            anonymous.typed,
            TypedProviderEvent::AuditOnly { .. }
        ));
    }

    #[test]
    fn only_the_control_response_can_bind_the_primary_session() {
        let mut scope = AgentScope::default();
        let mut early = event(
            serde_json::json!({"method":"thread/started","params":{"thread":{"id":"parent"}}}),
            1,
        );
        scope.apply(&mut early);
        assert!(matches!(early.typed, TypedProviderEvent::AuditOnly { .. }));
        let mut response = event(serde_json::json!({"result":{"thread":{"id":"parent"}}}), 2);
        scope.apply(&mut response);
        assert!(
            matches!(response.typed, TypedProviderEvent::SessionObserved(ref id) if id == "parent")
        );
        let mut child = event(
            serde_json::json!({"method":"thread/started","params":{"thread":{"id":"child"}}}),
            3,
        );
        scope.apply(&mut child);
        assert!(matches!(child.typed, TypedProviderEvent::AuditOnly { .. }));
        let mut parent = event(
            serde_json::json!({"method":"item/agentMessage/delta","params":{"threadId":"parent","itemId":"m","delta":"text"}}),
            4,
        );
        scope.apply(&mut parent);
        assert!(matches!(
            parent.typed,
            TypedProviderEvent::MessageDelta { .. }
        ));
    }
}
