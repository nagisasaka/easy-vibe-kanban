import { useState } from "react";
import type { Pipeline } from "shared/types";
import { CardContextSection } from "../../../../packages/web-core/src/features/pipeline/ui/CardContextSection";
import {
  defaultCardContext,
  replaceCardDescription,
  splitCardContext,
} from "../../../../packages/web-core/src/features/pipeline/model/cardContext";
import { composePipelineBlock } from "../../../../packages/web-core/src/features/pipeline/model/cardPipeline";

const wiki: Pipeline = {
  id: "wikillm",
  name: "LLM Wiki",
  description: null,
  stages: [
    {
      id: "recall",
      label: "Recall",
      prompt_fragment: "Consult prior knowledge.",
      default_enabled: true,
      heavy: false,
    },
  ],
};

export function CardContextHarness() {
  const legacy = new URLSearchParams(window.location.search).has("legacy");
  const [description, setDescription] = useState(() =>
    legacy
      ? `Existing task\n\n${composePipelineBlock(wiki, ["recall"])}`
      : defaultCardContext("New task", [wiki]),
  );
  return (
    <main>
      <textarea
        aria-label="Task description"
        value={splitCardContext(description).description}
        onChange={(event) =>
          setDescription(
            replaceCardDescription(description, event.target.value),
          )
        }
      />
      <CardContextSection
        description={description}
        pipelines={[wiki]}
        onRetry={() => {}}
        onDescriptionChange={setDescription}
      />
      <output data-testid="stored-description">{description}</output>
    </main>
  );
}
