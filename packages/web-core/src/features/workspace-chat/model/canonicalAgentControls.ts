import { agentRunsApi } from '@/shared/lib/agentRunApi';
import type { AgentGoalStatus, QuestionAnswer } from 'shared/types';

export interface CanonicalAgentControlClient {
  cancel(agentRunId: string, reason: string): Promise<unknown>;
  interruptTurn(agentRunId: string): Promise<unknown>;
  steer(agentRunId: string, content: string): Promise<unknown>;
  updateGoal(
    agentRunId: string,
    update: { objective?: string; status?: AgentGoalStatus }
  ): Promise<unknown>;
  updatePlanGoalDraft(agentRunId: string, objective: string): Promise<unknown>;
  clearGoal(agentRunId: string): Promise<unknown>;
  submitInput(
    agentRunId: string,
    inputId: string,
    content: string
  ): Promise<unknown>;
  resolveApproval(
    agentRunId: string,
    approvalId: string,
    approved: boolean,
    reason?: string
  ): Promise<unknown>;
}

export function createCanonicalAgentControls(
  client: CanonicalAgentControlClient
) {
  return {
    cancel: (agentRunId: string, reason: string) =>
      client.cancel(agentRunId, reason),
    interruptTurn: (agentRunId: string) => client.interruptTurn(agentRunId),
    steer: (agentRunId: string, content: string) =>
      client.steer(agentRunId, content),
    updateGoal: (
      agentRunId: string,
      update: { objective?: string; status?: AgentGoalStatus }
    ) => client.updateGoal(agentRunId, update),
    updatePlanGoalDraft: (agentRunId: string, objective: string) =>
      client.updatePlanGoalDraft(agentRunId, objective),
    clearGoal: (agentRunId: string) => client.clearGoal(agentRunId),
    submitInput: (agentRunId: string, inputId: string, content: string) =>
      client.submitInput(agentRunId, inputId, content),
    approve: (agentRunId: string, approvalId: string) =>
      client.resolveApproval(agentRunId, approvalId, true),
    deny: (agentRunId: string, approvalId: string, reason?: string) =>
      client.resolveApproval(agentRunId, approvalId, false, reason),
  };
}

export function serializeCanonicalInputAnswers(answers: QuestionAnswer[]) {
  return JSON.stringify({
    answers: answers.map(({ question, answer }) => ({ question, answer })),
  });
}

export const canonicalAgentControls =
  createCanonicalAgentControls(agentRunsApi);
