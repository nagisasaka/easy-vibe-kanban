import { describe, expect, it } from 'vitest';
import type { Pipeline } from 'shared/types';
import { buildWorkspaceCreatePrompt } from '@/shared/lib/workspaceCreateState';
import {
  composePipelineBlock,
  hasWikiLlmPipeline,
  updatePipelineBlock,
} from './cardPipeline';
import {
  cardContextPreview,
  defaultCardContext,
  hasSharedDirectories,
  replaceCardDescription,
  SHARED_DIRECTORIES_CONTEXT,
  splitCardContext,
  toggleSharedDirectories,
  withCardContext,
} from './cardContext';

const wiki: Pipeline = {
  id: 'wikillm',
  name: 'LLM Wiki',
  description: null,
  stages: [
    {
      id: 'recall',
      label: 'Recall',
      prompt_fragment: 'Consult prior knowledge.',
      default_enabled: true,
      heavy: false,
    },
    {
      id: 'enrich',
      label: 'Enrich',
      prompt_fragment: 'Record durable knowledge.',
      default_enabled: true,
      heavy: false,
    },
  ],
};

describe('card context', () => {
  it('refreshes old shared paths only when the preset is explicitly toggled', () => {
    const oldShared = SHARED_DIRECTORIES_CONTEXT.replaceAll(
      '.evk-shared/',
      '.evk/'
    );
    const wikiBlock = composePipelineBlock(wiki, ['recall']);
    const oldCard = withCardContext('Task', `${wikiBlock}\n\n${oldShared}`);
    expect(defaultCardContext(oldCard, [wiki])).toBe(oldCard);
    const context = splitCardContext(oldCard).context;
    const refreshed = toggleSharedDirectories(
      toggleSharedDirectories(context, false),
      true
    );
    expect(refreshed).toContain('.evk-shared/cache/');
    expect(refreshed).toContain('.evk-shared/persistent/');
    expect(refreshed).not.toContain('.evk/');
    expect(refreshed).toContain(wikiBlock);
    expect(
      splitCardContext(withCardContext(oldCard, refreshed)).description
    ).toBe('Task');
  });

  it('passes shared directories without reactivating Wiki through the existing initial workspace request', () => {
    const stored = defaultCardContext('Implement CRUD', [wiki]);
    const prompt = buildWorkspaceCreatePrompt('Feature', stored);
    expect(prompt).toBe(`Feature\n\n${stored}`);
    expect(hasWikiLlmPipeline(prompt!)).toBe(false);
    expect(prompt).toContain(SHARED_DIRECTORIES_CONTEXT);
    expect(
      splitCardContext(JSON.parse(JSON.stringify(stored))).description
    ).toBe('Implement CRUD');
  });

  it('defaults shared directories only while keeping the task editor free of generated text', () => {
    const stored = defaultCardContext('Build a feature', [wiki]);
    const split = splitCardContext(stored);
    expect(split.description).toBe('Build a feature');
    expect(hasWikiLlmPipeline(stored)).toBe(false);
    expect(hasSharedDirectories(split.context)).toBe(true);
    expect(defaultCardContext(stored, [wiki])).toBe(stored);
    expect(cardContextPreview(split.context)).not.toContain('<!--');
    expect(cardContextPreview(split.context)).not.toContain(
      'Consult prior knowledge.'
    );
  });

  it('keeps explicit opt-outs on draft reopening and description edits', () => {
    const disabled = withCardContext('Task', '');
    expect(defaultCardContext(disabled, [wiki])).toBe(disabled);
    const edited = replaceCardDescription(disabled, 'New task');
    expect(splitCardContext(defaultCardContext(edited, [wiki]))).toEqual({
      description: 'New task',
      context: '',
    });
  });

  it('reads legacy cards and preserves their exact instructions on description edits', () => {
    const block = composePipelineBlock(wiki, ['recall']);
    const old = `Before\n\n${block}\n\nAfter`;
    expect(splitCardContext(old)).toEqual({
      description: 'Before\n\nAfter',
      context: block,
    });
    expect(defaultCardContext(old, [wiki])).toBe(old);
    expect(replaceCardDescription(old, 'Edited')).toBe(`Edited\n\n${block}`);
    expect(splitCardContext('plain existing card').context).toBe('');
  });

  it('preserves stored context even if the preset definition changes', () => {
    const original = withCardContext(
      'Task',
      composePipelineBlock(wiki, ['recall'])
    );
    const changed = { ...wiki, stages: [] };
    expect(
      splitCardContext(
        defaultCardContext(replaceCardDescription(original, 'Edit'), [changed])
      ).context
    ).toBe(splitCardContext(original).context);
  });

  it('toggles shared directories independently of Wiki and never duplicates them', () => {
    const context = toggleSharedDirectories(
      composePipelineBlock(wiki, ['recall']),
      true
    );
    const noWiki = updatePipelineBlock(context, null, [], [wiki]);
    expect(hasSharedDirectories(noWiki)).toBe(true);
    expect(hasWikiLlmPipeline(noWiki)).toBe(false);
    const noShared = toggleSharedDirectories(context, false);
    expect(hasWikiLlmPipeline(noShared)).toBe(true);
    expect(
      toggleSharedDirectories(toggleSharedDirectories('', true), true)
    ).toBe(SHARED_DIRECTORIES_CONTEXT);
  });

  it('does not interpret inline marker mentions or incomplete blocks as context', () => {
    for (const description of [
      'Explain <!-- evk:card-context:start --> here',
      '<!-- evk:card-context:start -->\nUnfinished',
    ]) {
      expect(splitCardContext(description)).toEqual({
        description,
        context: '',
      });
    }
  });
});
