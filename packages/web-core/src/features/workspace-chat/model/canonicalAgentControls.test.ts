import { describe, expect, it, vi } from 'vitest';
import {
  createCanonicalAgentControls,
  serializeCanonicalInputAnswers,
  type CanonicalAgentControlClient,
} from './canonicalAgentControls';
import { AgentGoalStatus } from 'shared/types';

describe('canonical AgentRun controls', () => {
  it('dispatches cancel, input, and approval only through AgentRun methods', async () => {
    const client: CanonicalAgentControlClient = {
      cancel: vi.fn(async () => undefined),
      interruptTurn: vi.fn(async () => undefined),
      steer: vi.fn(async () => undefined),
      updateGoal: vi.fn(async () => undefined),
      updatePlanGoalDraft: vi.fn(async () => undefined),
      clearGoal: vi.fn(async () => undefined),
      submitInput: vi.fn(async () => undefined),
      resolveApproval: vi.fn(async () => undefined),
    };
    const controls = createCanonicalAgentControls(client);

    await controls.cancel('run-1', 'user request');
    await controls.submitInput('run-1', 'input-1', 'continue');
    await controls.approve('run-1', 'approval-1');
    await controls.deny('run-1', 'approval-2', 'needs changes');
    await controls.steer('run-1', 'Prefer the existing service layer.');
    await controls.updateGoal('run-1', {
      status: AgentGoalStatus.paused,
    });
    await controls.updatePlanGoalDraft('run-1', 'Edited objective');
    await controls.interruptTurn('run-1');
    await controls.clearGoal('run-1');

    expect(client.cancel).toHaveBeenCalledWith('run-1', 'user request');
    expect(client.submitInput).toHaveBeenCalledWith(
      'run-1',
      'input-1',
      'continue'
    );
    expect(client.resolveApproval).toHaveBeenNthCalledWith(
      1,
      'run-1',
      'approval-1',
      true
    );
    expect(client.resolveApproval).toHaveBeenNthCalledWith(
      2,
      'run-1',
      'approval-2',
      false,
      'needs changes'
    );
    expect(client.steer).toHaveBeenCalledWith(
      'run-1',
      'Prefer the existing service layer.'
    );
    expect(client.updateGoal).toHaveBeenCalledWith('run-1', {
      status: AgentGoalStatus.paused,
    });
    expect(client.updatePlanGoalDraft).toHaveBeenCalledWith(
      'run-1',
      'Edited objective'
    );
    expect(client.interruptTurn).toHaveBeenCalledWith('run-1');
    expect(client.clearGoal).toHaveBeenCalledWith('run-1');
  });

  it('serializes every question and answer without flattening multi-question input', () => {
    expect(
      serializeCanonicalInputAnswers([
        { question: 'Language?', answer: ['Rust'] },
        { question: 'Targets?', answer: ['CLI', 'Web'] },
      ])
    ).toBe(
      '{"answers":[{"question":"Language?","answer":["Rust"]},{"question":"Targets?","answer":["CLI","Web"]}]}'
    );
  });
});
