import type { Pipeline } from 'shared/types';
import {
  composePipelineBlock,
  extractPipelineBlock,
  removePipelineBlock,
} from './cardPipeline';

const START = '<!-- evk:card-context:start -->';
const END = '<!-- evk:card-context:end -->';
const BLOCK =
  /^<!-- evk:card-context:start -->\r?\n[\s\S]*?^<!-- evk:card-context:end -->[ \t]*$/gm;
const SHARED_BLOCK =
  /^<!-- evk:shared-directories:start -->\r?\n[\s\S]*?^<!-- evk:shared-directories:end -->[ \t]*$/gm;

export const SHARED_DIRECTORIES_CONTEXT = `<!-- evk:shared-directories:start -->
## Shared directories

EVK provides Git-untracked shared directories in each attached repository:
- .evk-shared/persistent/: local files retained across workspaces until explicitly deleted.
- .evk-shared/cache/: reproducible build outputs and caches.

These directories are shared by workspaces of the same repository. Changes affect other workspaces; avoid conflicting writes and do not clear a cache while another process uses it. Workspace deletion does not delete their contents. Use them when relevant, never commit their contents, and do not expose secrets. Tool-specific cache configuration is not automatic. Direct-folder workspaces may not provide these paths; check that they exist before using them.
<!-- evk:shared-directories:end -->`;

export function splitCardContext(description: string): {
  description: string;
  context: string;
} {
  const match = [...description.matchAll(BLOCK)].at(-1);
  if (match && match.index !== undefined) {
    return {
      description: join(
        description.slice(0, match.index),
        description.slice(match.index + match[0].length)
      ),
      context: match[0].slice(START.length, match[0].lastIndexOf(END)).trim(),
    };
  }
  // Existing cards retain the exact legacy Wiki instructions, without migration.
  return {
    description: removePipelineBlock(description),
    context: extractPipelineBlock(description),
  };
}

function join(before: string, after: string): string {
  return [before.trimEnd(), after.trimStart()].filter(Boolean).join('\n\n');
}

export function withCardContext(description: string, context: string): string {
  return join(
    splitCardContext(description).description,
    `${START}\n${context.trim()}\n${END}`
  );
}

export function replaceCardDescription(
  original: string,
  description: string
): string {
  const { context } = splitCardContext(original);
  if (!context && !original.includes(START)) return description;
  // Editing the task must not regenerate the stored instructions.
  const stored = [...original.matchAll(BLOCK)].at(-1)?.[0] ?? context;
  return join(description, stored);
}

export function hasSharedDirectories(context: string): boolean {
  return [...context.matchAll(SHARED_BLOCK)].length > 0;
}

export function toggleSharedDirectories(
  context: string,
  enabled: boolean
): string {
  const remaining = context.replace(SHARED_BLOCK, '').trim();
  return enabled ? join(remaining, SHARED_DIRECTORIES_CONTEXT) : remaining;
}

export function defaultCardContext(
  description: string,
  pipelines: readonly Pipeline[]
): string {
  if (description.includes(START) || extractPipelineBlock(description))
    return description;
  const wiki = pipelines.find((pipeline) => pipeline.id === 'wikillm');
  const wikiBlock = wiki
    ? composePipelineBlock(
        wiki,
        wiki.stages
          .filter((stage) => stage.default_enabled)
          .map((stage) => stage.id)
      )
    : '';
  return withCardContext(
    description,
    join(wikiBlock, SHARED_DIRECTORIES_CONTEXT)
  );
}

export function cardContextPreview(context: string): string {
  return context
    .replace(
      /^<!-- (?:vk:pipeline|evk:shared-directories)[^\n]*-->\r?\n?/gm,
      ''
    )
    .trim();
}
