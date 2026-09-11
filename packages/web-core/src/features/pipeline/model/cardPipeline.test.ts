import { describe, expect, it } from 'vitest';
import type { Pipeline } from 'shared/types';
import {
  PIPELINE_END,
  PIPELINE_START,
  composePipelineBlock,
  extractPipelineBlock,
  extractManualPipelineLines,
  inferPipelineSelection,
  removePipelineBlock,
  updatePipelineBlock,
} from './cardPipeline';

const pipeline: Pipeline = {
  id: 'wikillm',
  name: 'LLM Wiki',
  description: null,
  stages: [
    {
      id: 'recall',
      label: 'Recall prior knowledge',
      prompt_fragment: 'Recall.',
      default_enabled: true,
      heavy: false,
    },
    {
      id: 'enrich',
      label: 'Enrich knowledge base',
      prompt_fragment: 'Enrich.',
      default_enabled: true,
      heavy: false,
    },
  ],
};

describe('card pipeline block', () => {
  it('preserves stage order and is idempotent', () => {
    const once = updatePipelineBlock('Manual text', pipeline, [
      'recall',
      'enrich',
    ]);
    const twice = updatePipelineBlock(once, pipeline, ['recall', 'enrich']);
    expect(twice).toBe(once);
    expect(once.indexOf('Recall.')).toBeLessThan(once.indexOf('Enrich.'));
  });

  it('replaces or removes only the generated block', () => {
    const initial = updatePipelineBlock('Before\n\nAfter', pipeline, [
      'recall',
      'enrich',
    ]);
    expect(removePipelineBlock(initial)).toBe('Before\n\nAfter');
    expect(updatePipelineBlock(initial, pipeline, ['enrich'])).toContain(
      'Enrich.'
    );
    expect(updatePipelineBlock(initial, pipeline, ['enrich'])).not.toContain(
      'Recall.'
    );
  });

  it('preserves manual additions made inside a generated block', () => {
    const initial = composePipelineBlock(pipeline, [
      'recall',
      'enrich',
    ]).replace(
      PIPELINE_END,
      `Operator note: keep this constraint.\n${PIPELINE_END}`
    );
    expect(extractManualPipelineLines(initial, [pipeline])).toEqual([
      'Operator note: keep this constraint.',
    ]);
    const updated = updatePipelineBlock(
      initial,
      pipeline,
      ['recall'],
      [pipeline]
    );
    expect(updated).toContain('Operator note: keep this constraint.');
    expect(updated).not.toContain('Enrich.');
    expect(updatePipelineBlock(initial, null, [], [pipeline])).toBe(
      'Operator note: keep this constraint.'
    );
  });

  it('does not preserve obsolete generated prompts after a definition update', () => {
    const initial = composePipelineBlock(pipeline, ['recall']);
    const updatedPipeline = {
      ...pipeline,
      stages: pipeline.stages.map((stage) =>
        stage.id === 'recall'
          ? { ...stage, prompt_fragment: 'Recall with the new rules.' }
          : stage
      ),
    };
    const updated = updatePipelineBlock(
      initial,
      updatedPipeline,
      ['recall'],
      [updatedPipeline]
    );
    expect(updated).toContain('Recall with the new rules.');
    expect(updated).not.toContain('**Recall prior knowledge:** Recall.');
  });

  it('does not interpret marker text embedded in prose', () => {
    const prose = `quote ${PIPELINE_START} here and ${PIPELINE_END} there`;
    expect(removePipelineBlock(prose)).toBe(prose);
  });

  it('extracts the last complete block and infers selected stages', () => {
    const old = composePipelineBlock(pipeline, ['recall']);
    const current = composePipelineBlock(pipeline, ['enrich']);
    const description = `${old}\nmanual\n${current}`;
    expect(extractPipelineBlock(description)).toBe(current);
    expect(inferPipelineSelection(description, [pipeline])).toEqual({
      pipelineId: 'wikillm',
      stageIds: new Set(['enrich']),
    });
  });

  it('leaves ordinary cards unchanged', () => {
    expect(removePipelineBlock('ordinary')).toBe('ordinary');
    expect(updatePipelineBlock('ordinary', null, [])).toBe('ordinary');
  });
});
