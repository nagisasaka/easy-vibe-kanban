import { describe, expect, it } from 'vitest';
import {
  wikiChatDraft,
  buildWikiBootstrapPrompt,
  prepareWikiDraft,
  registerWikiComposer,
} from './wikiBootstrap';

describe('Wiki bootstrap composer handoff', () => {
  it('enables Codex Goal and exits Plan without changing other permission policies', () => {
    expect(wikiChatDraft('Create wiki', 'CODEX', 'PLAN')).toEqual({
      text: 'Create wiki',
      goal: true,
      overrides: { execution_mode: 'goal', permission_policy: 'SUPERVISED' },
    });
    expect(
      wikiChatDraft('Create wiki', 'CODEX', 'SUPERVISED').overrides
    ).toEqual({ execution_mode: 'goal' });
    expect(wikiChatDraft('Create wiki', 'CLAUDE_CODE', 'PLAN')).toEqual({
      text: 'Create wiki',
      goal: false,
      overrides: {},
    });
  });
  it('does not queue a request for a missing or different workspace', () => {
    const unregister = registerWikiComposer('one', () => 'goal');
    expect(prepareWikiDraft('two', 'prompt', false)).toBe('unavailable');
    unregister();
    expect(prepareWikiDraft('one', 'prompt', false)).toBe('unavailable');
  });

  it('preserves drafts until replacement is explicitly requested', () => {
    let text = 'Existing user draft';
    const unregister = registerWikiComposer('draft', (prompt, replace) => {
      if (text && !replace) return 'occupied';
      text = prompt;
      return 'plain';
    });
    expect(prepareWikiDraft('draft', 'Wiki request', false)).toBe('occupied');
    expect(text).toBe('Existing user draft');
    expect(prepareWikiDraft('draft', 'Wiki request', true)).toBe('plain');
    expect(text).toBe('Wiki request');
    unregister();
  });

  it('rejects ambiguous composers rather than modifying multiple sessions', () => {
    const first = registerWikiComposer('multi', () => {
      throw new Error('Must not run');
    });
    const second = registerWikiComposer('multi', () => 'goal');
    expect(prepareWikiDraft('multi', 'prompt', false)).toBe('unavailable');
    first();
    expect(prepareWikiDraft('multi', 'prompt', false)).toBe('goal');
    second();
  });

  it('includes target, language, investigation scope and completion criteria in visible text', () => {
    const prompt = buildWikiBootstrapPrompt({
      repository: 'repo-a',
      language: 'ja',
      instructions: 'Exclude legacy/',
    });
    expect(prompt).toContain('"repo-a"');
    expect(prompt).toContain('output_language = "ja"');
    expect(prompt).toContain('Exclude legacy/');
    expect(prompt).toContain('Finish when');
    expect(prompt).toContain('without repairing or overwriting');
    expect(prompt).toContain('Do not commit, push, publish');
  });

  it('requires bounded read-only delegation and an honest self-review fallback', () => {
    const prompt = buildWikiBootstrapPrompt({
      repository: 'repo',
      language: 'ja',
      instructions: '',
    });
    expect(prompt).toContain('within the configured concurrency limit');
    expect(prompt).toContain('Do not change that limit');
    expect(prompt).toContain('primary agent owns all Wiki writes');
    expect(prompt).toContain('subagents are read-only');
    expect(prompt).toContain('reviewers who did not author the draft');
    expect(prompt).toContain('If delegation is disabled or unavailable');
    expect(wikiChatDraft(prompt, 'CODEX', 'AUTO').overrides).toEqual({
      execution_mode: 'goal',
    });
  });

  it('gates completion on evidence, development questions and a review cycle', () => {
    const prompt = buildWikiBootstrapPrompt({
      repository: 'repo',
      language: 'en',
      instructions: '',
    });
    expect(prompt).toContain('repo/HEAD/diff');
    expect(prompt).toContain('The first draft is not completion');
    expect(prompt).toContain('at least one complete review cycle');
    expect(prompt).toContain('accuracy and development-utility');
    expect(prompt).toContain('Try answering them using the Wiki first');
    expect(prompt).toContain('re-review changed claims and important findings');
    expect(prompt).toContain(
      'no significant errors or actionable critical gaps'
    );
    expect(prompt).toContain('not quality targets');
    expect(prompt).toContain('Keep progress/review transcripts out of Wiki');
  });
  it('leaves room for executor constraints within the native Goal limit', () => {
    const prompt = buildWikiBootstrapPrompt({
      repository: 'easy-vibe-kanban',
      language: 'ja',
      instructions: '',
    });
    expect(Array.from(prompt).length).toBeLessThanOrEqual(3800);
  });
});
