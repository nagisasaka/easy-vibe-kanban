import React, { useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  AgentRunSessionProvider,
  useAgentRunSession,
} from "../../packages/web-core/src/features/agent-runtime/model/AgentRunSessionContext";
import { useAgentRunCanonicalStream } from "../../packages/web-core/src/features/agent-runtime/model/useAgentRunCanonicalStream";
import { useConversationHistory } from "../../packages/web-core/src/features/workspace-chat/model/hooks/useConversationHistory";
import { useConversationVirtualizer } from "../../packages/web-core/src/features/workspace-chat/model/useConversationVirtualizer";

const api = window as any;
api.sockets = [];
api.updates = [];
api.controllers = [];
api.canonical = {
  conversation: { entries: [], runCount: 1 },
  isLoading: false,
};
api.processes = {
  executionProcessesVisible: [],
  isLoading: false,
  isConnected: true,
};

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false } },
});
api.runs = [];
function SessionObserver() {
  api.session = useAgentRunSession();
  return null;
}
function Harness() {
  const [sessionMode, setSessionMode] = useState(false);
  api.openSession = () => setSessionMode(true);
  const [run, setRun] = useState("run-a");
  const [version, setVersion] = useState(0);
  api.setRun = setRun;
  api.render = () => setVersion((v) => v + 1);
  const stream = useAgentRunCanonicalStream(run);
  api.stream = stream;
  useConversationHistory({
    scopeKey: run,
    onTimelineUpdated: (source, type, loading) =>
      api.updates.push({
        type,
        loading,
        scripts: Object.keys(source.executionProcessState),
      }),
  } as any);
  const ref = useRef<HTMLDivElement>(null);
  const scroll = useConversationVirtualizer({
    rows: [],
    totalRowCount: version,
    scrollContainerRef: ref,
  });
  api.scroll = scroll;
  if (sessionMode)
    return (
      <QueryClientProvider client={queryClient}>
        <AgentRunSessionProvider sessionId="s">
          <SessionObserver />
        </AgentRunSessionProvider>
      </QueryClientProvider>
    );
  return (
    <div ref={ref} id="scroller" style={{ height: 300, overflow: "auto" }}>
      <div style={{ height: 3000 + version * 100 }}>history</div>
    </div>
  );
}
createRoot(document.getElementById("root")!).render(<Harness />);
