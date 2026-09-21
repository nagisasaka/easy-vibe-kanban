import { useCallback, useRef, useState } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AppRuntimeProvider } from "@/shared/hooks/useAppRuntime";
import { useSessionMessageEditor } from "@/features/workspace-chat/model/hooks/useSessionMessageEditor";
import { useSessionSend } from "@/features/workspace-chat/model/hooks/useSessionSend";
import { isSessionDraftSubmissionCurrent } from "@/features/workspace-chat/model/sessionDraft";
import { useJsonPatchWsStream } from "@/shared/hooks/useJsonPatchWsStream";
import type { ExecutorConfig } from "shared/types";

const config: ExecutorConfig = {
  executor: "CODEX",
  execution_mode: "goal",
  reasoning_id: "max",
  goal_max_concurrent_agents: 0,
};
const client = new QueryClient({
  defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
});

function Input() {
  const [id, setId] = useState("a");
  const [metadata, setMetadata] = useState("skill-a");
  const editor = useSessionMessageEditor({ scratchId: id });
  const sender = useSessionSend({
    sessionId: id === "new" ? undefined : id,
    workspaceId: "workspace-a",
    isNewSessionMode: id === "new",
    executorConfig: config,
  });
  const current = useRef({ scope: id, revision: 0, content: "" });
  current.current = {
    scope: id,
    revision: editor.getDraftRevision(),
    content: JSON.stringify([editor.localMessage, metadata]),
  };
  const submit = async () => {
    const snapshot = { ...current.current };
    const text = editor.localMessage;
    editor.cancelDebouncedSave();
    void editor.saveToScratch(text, config);
    const result = await sender.send(text);
    if (
      !result ||
      !isSessionDraftSubmissionCurrent(
        { ...current.current, revision: editor.getDraftRevision() },
        snapshot,
      )
    )
      return;
    editor.setLocalMessage("");
    await editor.clearDraft({
      type: "DRAFT_FOLLOW_UP",
      data: { message: text, executor_config: config },
    });
    if (result.createdSessionId) setId(result.createdSessionId);
  };
  return (
    <>
      <label>
        Identity
        <select value={id} onChange={(event) => setId(event.target.value)}>
          <option>a</option>
          <option>b</option>
          <option>new</option>
          <option>created</option>
        </select>
      </label>
      <label>
        Draft
        <textarea
          value={editor.localMessage}
          onChange={(event) =>
            editor.handleMessageChange(event.target.value, config)
          }
        />
      </label>
      <label>
        Skill
        <input
          value={metadata}
          onChange={(event) => setMetadata(event.target.value)}
        />
      </label>
      <button onClick={() => void submit()} disabled={sender.isSending}>
        Send
      </button>
      <output data-testid="send-error">{sender.error}</output>
      <output data-testid="scratch-ready">
        {String(editor.hasInitialValue)}
      </output>
    </>
  );
}

function Stream() {
  const [id, setId] = useState("a");
  const initialData = useCallback(() => ({ value: "" }), []);
  const result = useJsonPatchWsStream(`/api/fixture/${id}`, true, initialData);
  return (
    <>
      <button onClick={() => setId("b")}>Switch stream</button>
      <output data-testid="stream">{JSON.stringify(result)}</output>
    </>
  );
}

export function RuntimeInputHarness() {
  const params = new URLSearchParams(location.search);
  return (
    <QueryClientProvider client={client}>
      <AppRuntimeProvider runtime={params.has("local") ? "local" : "remote"}>
        {params.has("stream") ? <Stream /> : <Input />}
      </AppRuntimeProvider>
    </QueryClientProvider>
  );
}
