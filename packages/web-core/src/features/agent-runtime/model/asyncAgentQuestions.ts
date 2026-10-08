import type { AgentEventEnvelope, JsonValue } from 'shared/types';

export interface AsyncAgentQuestion {
  id: string;
  title: string;
  options: string[];
}

function object(value: JsonValue | undefined) {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value
    : null;
}

/** Questions are optional durable metadata, not blocking input controls. */
export function collectAsyncAgentQuestions(
  events: readonly AgentEventEnvelope[]
): AsyncAgentQuestion[] {
  const questions = new Map<string, AsyncAgentQuestion>();
  for (const event of events) {
    const payload = event.payload;
    if (
      payload.type !== 'provider_extension' ||
      payload.data.provider_namespace !== 'codex' ||
      payload.data.provider_event !== 'async_questions'
    )
      continue;
    const data = object(payload.data.payload);
    if (
      data?.schema_version !== 1 ||
      typeof data.message_id !== 'string' ||
      !Array.isArray(data.questions)
    )
      continue;
    data.questions.forEach((value, index) => {
      const question = object(value);
      if (typeof question?.title !== 'string' || !question.title.trim()) return;
      const id = `${event.run_attempt_id}:${data.message_id}:${index}`;
      questions.set(id, {
        id,
        title: question.title,
        options: Array.isArray(question.options)
          ? question.options.filter(
              (option): option is string => typeof option === 'string'
            )
          : [],
      });
    });
  }
  return [...questions.values()];
}

export function formatAsyncQuestionAnswer(
  question: AsyncAgentQuestion,
  answer: string
) {
  return `Question: ${question.title}\nAnswer: ${answer.trim()}`;
}

export async function deliverAsyncQuestionAnswer(
  question: AsyncAgentQuestion,
  answer: string,
  delivery: {
    activeRunId: string | null;
    steer: (runId: string, content: string) => Promise<unknown>;
    followUp: (content: string) => Promise<unknown>;
  }
): Promise<boolean> {
  const content = formatAsyncQuestionAnswer(question, answer);
  if (delivery.activeRunId) {
    // Never fall back after a failed request: delivery may already have happened.
    await delivery.steer(delivery.activeRunId, content);
    return true;
  }
  return Boolean(await delivery.followUp(content));
}
