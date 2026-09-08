import type { Pipeline } from 'shared/types';

export const PIPELINE_START = '<!-- vk:pipeline:start -->';
export const PIPELINE_END = '<!-- vk:pipeline:end -->';
const ORDER_INSTRUCTION =
  'These are declarative stages for the agent in this workspace. Consider them in the listed order; use judgement where a stage explicitly permits skipping work.';
const PIPELINE_ID_RE = /^<!-- vk:pipeline:id=([a-zA-Z0-9_-]+) -->$/m;
const PIPELINE_STAGE_RE = /^<!-- vk:pipeline:stage=([a-zA-Z0-9_-]+) -->$/gm;

const MARKER_BLOCK_RE = new RegExp(
  `^${escapeRegExp(PIPELINE_START)}\\r?\\n[\\s\\S]*?^${escapeRegExp(PIPELINE_END)}[ \\t]*$`,
  'gm'
);

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

export function extractPipelineBlock(description: string): string {
  const matches = [...description.matchAll(MARKER_BLOCK_RE)];
  return matches.at(-1)?.[0] ?? '';
}

export function removePipelineBlock(description: string): string {
  const matches = [...description.matchAll(MARKER_BLOCK_RE)];
  const match = matches.at(-1);
  if (!match || match.index === undefined) return description;
  const before = description.slice(0, match.index).trimEnd();
  const after = description.slice(match.index + match[0].length).trimStart();
  if (!before) return after;
  if (!after) return before;
  return `${before}\n\n${after}`;
}

export function extractManualPipelineLines(
  description: string,
  knownPipelines: readonly Pipeline[]
): string[] {
  const block = extractPipelineBlock(description);
  if (!block) return [];
  const generatedStages = new Set(
    knownPipelines.flatMap((pipeline) =>
      pipeline.stages.map(
        (stage) => `**${stage.label}:** ${stage.prompt_fragment}`
      )
    )
  );
  const lines = block.split(/\r?\n/).map((line) => line.trimEnd());
  return lines.filter((line, index) => {
    if (!line.trim()) return false;
    if (line === PIPELINE_START || line === PIPELINE_END) return false;
    if (/^## Pipeline\b/.test(line)) return false;
    if (/^<!-- vk:pipeline:(?:id|stage)=/.test(line)) return false;
    if (line === ORDER_INSTRUCTION) return false;
    // Metadata makes recomposition safe even when a TOML prompt changes:
    // the numbered line following a stage marker is generated, not a note.
    if (
      index > 0 &&
      /^<!-- vk:pipeline:stage=[a-zA-Z0-9_-]+ -->$/.test(lines[index - 1]) &&
      /^\d+\.\s+/.test(line)
    ) {
      return false;
    }
    const stage = line.match(/^\d+\.\s+(.*)$/)?.[1];
    return !stage || !generatedStages.has(stage);
  });
}

export function composePipelineBlock(
  pipeline: Pipeline,
  enabledStageIds: ReadonlySet<string> | readonly string[]
): string {
  const enabled =
    enabledStageIds instanceof Set ? enabledStageIds : new Set(enabledStageIds);
  const stages = pipeline.stages.filter((stage) => enabled.has(stage.id));
  if (stages.length === 0) return '';
  const lines = [
    PIPELINE_START,
    `<!-- vk:pipeline:id=${pipeline.id} -->`,
    `## Pipeline: ${pipeline.name}`,
    '',
    ORDER_INSTRUCTION,
    '',
    ...stages.flatMap((stage, index) => [
      `<!-- vk:pipeline:stage=${stage.id} -->`,
      `${index + 1}. **${stage.label}:** ${stage.prompt_fragment}`,
    ]),
    PIPELINE_END,
  ];
  return lines.join('\n');
}

export function updatePipelineBlock(
  description: string,
  pipeline: Pipeline | null,
  enabledStageIds: ReadonlySet<string> | readonly string[],
  knownPipelines: readonly Pipeline[] = pipeline ? [pipeline] : []
): string {
  const manualLines = extractManualPipelineLines(description, knownPipelines);
  const withoutBlock = removePipelineBlock(description);
  const preserved = manualLines.length
    ? [withoutBlock.trimEnd(), manualLines.join('\n')]
        .filter(Boolean)
        .join('\n\n')
    : withoutBlock;
  if (!pipeline) return preserved;
  const block = composePipelineBlock(pipeline, enabledStageIds);
  if (!block) return preserved;
  return preserved ? `${preserved.trimEnd()}\n\n${block}` : block;
}

export function inferPipelineSelection(
  description: string,
  pipelines: readonly Pipeline[]
): { pipelineId: string | null; stageIds: Set<string> } {
  const block = extractPipelineBlock(description);
  if (!block) return { pipelineId: null, stageIds: new Set() };
  const storedId = block.match(PIPELINE_ID_RE)?.[1];
  const pipeline =
    pipelines.find((candidate) => candidate.id === storedId) ??
    pipelines.find((candidate) =>
      block.includes(`## Pipeline: ${candidate.name}`)
    );
  if (!pipeline) return { pipelineId: null, stageIds: new Set() };
  const storedStages = new Set(
    [...block.matchAll(PIPELINE_STAGE_RE)].map((match) => match[1])
  );
  return {
    pipelineId: pipeline.id,
    stageIds: new Set(
      pipeline.stages
        .filter(
          (stage) =>
            storedStages.has(stage.id) ||
            (storedStages.size === 0 && block.includes(`**${stage.label}:**`))
        )
        .map((stage) => stage.id)
    ),
  };
}

export function hasWikiLlmPipeline(description: string): boolean {
  const block = extractPipelineBlock(description);
  return block.includes('## Pipeline: LLM Wiki');
}
