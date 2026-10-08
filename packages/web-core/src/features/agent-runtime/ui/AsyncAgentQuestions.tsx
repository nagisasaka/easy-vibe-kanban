import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AsyncAgentQuestion } from '../model/asyncAgentQuestions';

interface SavedAnswers {
  drafts: Record<string, string>;
  answered: string[];
}

function readAnswers(key: string): SavedAnswers {
  try {
    const saved = JSON.parse(localStorage.getItem(key) ?? 'null');
    if (
      saved &&
      typeof saved.drafts === 'object' &&
      saved.drafts !== null &&
      Array.isArray(saved.answered)
    ) {
      return {
        drafts: Object.fromEntries(
          Object.entries(saved.drafts).filter(
            (entry) => typeof entry[1] === 'string'
          )
        ) as Record<string, string>,
        answered: saved.answered.filter(
          (value: unknown): value is string => typeof value === 'string'
        ),
      };
    }
  } catch {
    /* Storage is optional; answering must still work. */
  }
  return { drafts: {}, answered: [] };
}

/** Mount with a session/host key so late answers cannot alter another inbox. */
export function AsyncAgentQuestions({
  questions,
  scope,
  disabled,
  onAnswer,
}: {
  questions: AsyncAgentQuestion[];
  scope: string;
  disabled: boolean;
  onAnswer: (question: AsyncAgentQuestion, answer: string) => Promise<void>;
}) {
  const { t } = useTranslation('common');
  const storageKey = `lvk:async-questions:${scope}`;
  const [saved, setSaved] = useState(() => readAnswers(storageKey));
  const savedRef = useRef(saved);
  const [expanded, setExpanded] = useState(false);
  const [submitting, setSubmitting] = useState<string | null>(null);
  const submissionLock = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const pending = questions.filter(
    (question) => !saved.answered.includes(question.id)
  );

  const save = (next: SavedAnswers) => {
    savedRef.current = next;
    setSaved(next);
    try {
      localStorage.setItem(storageKey, JSON.stringify(next));
    } catch {
      /* In-memory fallback. */
    }
  };
  const submit = async (question: AsyncAgentQuestion) => {
    const answer = savedRef.current.drafts[question.id]?.trim();
    if (!answer || disabled || submissionLock.current) return;
    submissionLock.current = true;
    setSubmitting(question.id);
    setError(null);
    try {
      await onAnswer(question, answer);
      // Only an accepted delivery consumes a question. Failed sends retain the draft.
      const next = {
        ...savedRef.current,
        drafts: { ...savedRef.current.drafts },
        answered: [...savedRef.current.answered, question.id],
      };
      delete next.drafts[question.id];
      save(next);
    } catch (cause) {
      setError(
        cause instanceof Error ? cause.message : t('asyncQuestions.sendFailed')
      );
    } finally {
      submissionLock.current = false;
      setSubmitting(null);
    }
  };

  if (!pending.length) return null;
  return (
    <section
      className="rounded-sm border border-border bg-panel text-normal"
      aria-label={t('asyncQuestions.title')}
    >
      <button
        type="button"
        className="w-full px-base py-half text-left"
        aria-expanded={expanded}
        onClick={() => setExpanded(!expanded)}
      >
        {t('asyncQuestions.pending', { count: pending.length })}
      </button>
      {expanded && (
        <div className="max-h-72 space-y-base overflow-y-auto p-base">
          <p className="text-sm text-low">{t('asyncQuestions.description')}</p>
          {pending.map((question) => (
            <div key={question.id} className="space-y-half">
              <label className="block" htmlFor={`async-answer-${question.id}`}>
                {question.title}
              </label>
              <div className="flex flex-wrap gap-half">
                {question.options.map((option, index) => (
                  <button
                    type="button"
                    key={index}
                    className="rounded-sm border border-border px-base py-half text-sm hover:bg-secondary"
                    aria-pressed={saved.drafts[question.id] === option}
                    disabled={submitting === question.id}
                    onClick={() =>
                      save({
                        ...savedRef.current,
                        drafts: {
                          ...savedRef.current.drafts,
                          [question.id]: option,
                        },
                      })
                    }
                  >
                    {option}
                  </button>
                ))}
              </div>
              <textarea
                id={`async-answer-${question.id}`}
                className="w-full rounded-sm border border-border bg-primary p-half"
                placeholder={t('asyncQuestions.placeholder')}
                value={saved.drafts[question.id] ?? ''}
                disabled={submitting === question.id}
                onChange={(event) =>
                  save({
                    ...savedRef.current,
                    drafts: {
                      ...savedRef.current.drafts,
                      [question.id]: event.target.value,
                    },
                  })
                }
              />
              <button
                type="button"
                className="rounded-sm border border-border px-base py-half disabled:opacity-50"
                disabled={
                  disabled ||
                  submitting !== null ||
                  !saved.drafts[question.id]?.trim()
                }
                onClick={() => void submit(question)}
              >
                {submitting === question.id
                  ? t('asyncQuestions.sending')
                  : t('asyncQuestions.send')}
              </button>
            </div>
          ))}
          {error && (
            <p role="alert" className="text-error">
              {error}
            </p>
          )}
        </div>
      )}
    </section>
  );
}
