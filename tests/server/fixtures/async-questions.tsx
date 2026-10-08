import { useState } from "react";
import { createRoot } from "react-dom/client";
import type { AgentEventEnvelope } from "shared/types";
import { AsyncAgentQuestions } from "@/features/agent-runtime/ui/AsyncAgentQuestions";
import {
  collectAsyncAgentQuestions,
  deliverAsyncQuestionAnswer,
} from "@/features/agent-runtime/model/asyncAgentQuestions";

const event: AgentEventEnvelope = {
  schema_version: 1,
  payload_version: 1,
  event_id: "async-test",
  session_id: "test",
  agent_run_id: "test",
  turn_id: "test",
  run_attempt_id: "test",
  run_attempt_number: 1,
  sequence: 1n,
  correlation_id: "test",
  timestamp: "2026-10-08T00:00:00Z",
  native_refs: [],
  payload: {
    type: "provider_extension",
    data: {
      provider_namespace: "codex",
      provider_event: "async_questions",
      payload: {
        schema_version: 1,
        message_id: "test-message",
        questions: [
          { title: "Which colour?", options: ["Blue", "Green"] },
          { title: "Any constraints?", options: [] },
        ],
      },
    },
  },
};
const questions = collectAsyncAgentQuestions([event]);

async function deliver(endpoint: string, content: string) {
  const response = await fetch(`/__async-question-test/${endpoint}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ content }),
  });
  if (!response.ok) throw new Error("Test delivery rejected");
  return {};
}

function Harness() {
  const [running, setRunning] = useState(true);
  const [progress, setProgress] = useState(0);
  const [session, setSession] = useState("a");
  return (
    <section aria-label="Async question regression">
      <label>
        Normal message
        <textarea />
      </label>
      <button onClick={() => setProgress(progress + 1)}>Continue work</button>
      <output aria-label="Work progress">{progress}</output>
      <button onClick={() => setRunning(false)}>Finish run</button>
      <button onClick={() => setSession(session === "a" ? "b" : "a")}>
        Switch session
      </button>
      <AsyncAgentQuestions
        key={session}
        scope={`regression:${session}`}
        questions={questions}
        disabled={false}
        onAnswer={async (question, answer) => {
          const accepted = await deliverAsyncQuestionAnswer(question, answer, {
            activeRunId: running ? "running" : null,
            steer: (_id, content) => deliver("steer", content),
            followUp: (content) => deliver("follow-up", content),
          });
          if (!accepted) throw new Error("Test delivery rejected");
        }}
      />
    </section>
  );
}

export function mount() {
  const host = document.createElement("div");
  host.className = "new-design";
  host.style.cssText =
    "position:fixed;inset:40px;z-index:99999;background:white;padding:24px;overflow:auto";
  document.body.append(host);
  const root = createRoot(host);
  root.render(<Harness />);
  return () => {
    root.unmount();
    host.remove();
  };
}
