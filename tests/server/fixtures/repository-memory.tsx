import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { Repo } from "shared/types";
import { createMachineClient } from "@/shared/lib/machineClient";
import { RepositoryMemorySettings } from "@/shared/dialogs/settings/settings/RepositoryMemorySettings";

export function mount() {
  const host = document.createElement("div");
  host.className = "new-design";
  host.style.cssText =
    "position:fixed;inset:40px;z-index:99999;background:white;padding:24px;overflow:auto";
  document.body.append(host);
  const root = createRoot(host);
  const query = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = createMachineClient("local", {
    id: "local",
    apiHostId: null,
    kind: "local",
    label: "Local",
  });
  root.render(
    <QueryClientProvider client={query}>
      <section aria-label="Repository memory regression">
        <RepositoryMemorySettings
          repo={
            { id: "adoption-regression", default_target_branch: "main" } as Repo
          }
          client={client}
        />
      </section>
    </QueryClientProvider>,
  );
  return () => {
    root.unmount();
    query.clear();
    host.remove();
  };
}
