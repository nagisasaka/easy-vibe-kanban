import { useEffect, useState } from 'react';
import { AgentGoalStatus, type AgentGoalState } from 'shared/types';
import { watchGoalElapsed } from '../model/goalElapsed';

interface GoalProgressCardProps {
  goal: AgentGoalState;
  isRunActive: boolean;
  busy?: boolean;
  controllable?: boolean;
  error?: string | null;
  onPause: () => void;
  onResume: () => void;
  onResumeSaved?: () => void;
  resumeSavedDisabled?: boolean;
  onEdit: (objective: string) => void;
  onClear: () => void;
  onInterruptTurn: () => void;
}

const STATUS_LABELS: Record<AgentGoalStatus, string> = {
  [AgentGoalStatus.active]: 'Active',
  [AgentGoalStatus.paused]: 'Paused',
  [AgentGoalStatus.blocked]: 'Blocked',
  [AgentGoalStatus.usageLimited]: 'Usage limited',
  [AgentGoalStatus.budgetLimited]: 'Budget limited',
  [AgentGoalStatus.complete]: 'Complete',
};

function formatDuration(totalSeconds: number) {
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  return hours > 0
    ? `${hours}h ${minutes}m ${seconds}s`
    : `${minutes}m ${seconds}s`;
}

export function GoalProgressCard({
  goal,
  isRunActive,
  busy = false,
  controllable = true,
  error,
  onPause,
  onResume,
  onResumeSaved,
  resumeSavedDisabled = false,
  onEdit,
  onClear,
  onInterruptTurn,
}: GoalProgressCardProps) {
  const [isEditing, setIsEditing] = useState(false);
  const [draft, setDraft] = useState(goal.objective);
  const [elapsed, setElapsed] = useState(Number(goal.time_used_seconds));

  useEffect(() => {
    setDraft(goal.objective);
  }, [goal.objective]);

  useEffect(() => {
    return watchGoalElapsed(
      Number(goal.time_used_seconds),
      goal.status === AgentGoalStatus.active,
      isRunActive,
      setElapsed
    );
  }, [goal.status, goal.time_used_seconds, isRunActive]);

  const canResume = [
    AgentGoalStatus.paused,
    AgentGoalStatus.blocked,
    AgentGoalStatus.usageLimited,
    AgentGoalStatus.budgetLimited,
  ].includes(goal.status);
  const buttonClass =
    'border-border hover:bg-secondary rounded-sm border px-base py-half text-xs text-normal disabled:cursor-not-allowed disabled:opacity-50';

  return (
    <section className="border-border bg-primary mb-base rounded-md border p-base">
      <div className="flex flex-wrap items-center justify-between gap-base">
        <div className="flex items-center gap-base">
          <span className="text-sm font-medium text-high">
            {isRunActive ? 'Goal' : 'Saved Goal'}
          </span>
          <span className="bg-secondary rounded-full px-base py-half text-xs text-normal">
            {isRunActive ? STATUS_LABELS[goal.status] : 'No active run'}
          </span>
        </div>
        <div className="flex flex-wrap gap-base text-xs text-low">
          <span>
            Tokens: {String(goal.tokens_used)} /{' '}
            {goal.token_budget == null ? 'Auto' : String(goal.token_budget)}
          </span>
          <span>
            {isRunActive ? 'Elapsed' : 'Recorded time'}:{' '}
            {formatDuration(
              isRunActive ? elapsed : Number(goal.time_used_seconds)
            )}
          </span>
        </div>
      </div>

      {!isRunActive && (
        <p className="mt-base text-xs text-low">
          No agent is running for this Goal. Last recorded Goal status:{' '}
          {STATUS_LABELS[goal.status]}. The time shown is the last reported
          value.
        </p>
      )}

      {isEditing && isRunActive && controllable ? (
        <div className="mt-base space-y-base">
          <textarea
            value={draft}
            onChange={(event) => setDraft(event.currentTarget.value)}
            rows={5}
            className="border-border bg-secondary text-normal w-full resize-y rounded-sm border p-base text-sm"
          />
          <p className="text-xs text-warning">
            Replacing the objective resets Codex Goal usage accounting.
          </p>
          <div className="flex justify-end gap-base">
            <button
              type="button"
              className={buttonClass}
              onClick={() => {
                setDraft(goal.objective);
                setIsEditing(false);
              }}
            >
              Cancel
            </button>
            <button
              type="button"
              className={buttonClass}
              disabled={busy || !draft.trim() || draft === goal.objective}
              onClick={() => {
                onEdit(draft.trim());
                setIsEditing(false);
              }}
            >
              Save Goal
            </button>
          </div>
        </div>
      ) : (
        <p className="mt-base max-h-24 overflow-y-auto whitespace-pre-wrap text-sm text-normal">
          {goal.objective}
        </p>
      )}

      {!isEditing && controllable && isRunActive && (
        <div className="mt-base flex flex-wrap gap-base">
          {goal.status === AgentGoalStatus.active && (
            <button
              type="button"
              className={buttonClass}
              disabled={busy}
              onClick={onPause}
            >
              Pause
            </button>
          )}
          {canResume && (
            <button
              type="button"
              className={buttonClass}
              disabled={busy}
              onClick={onResume}
            >
              Resume
            </button>
          )}
          <button
            type="button"
            className={buttonClass}
            disabled={busy}
            onClick={() => setIsEditing(true)}
          >
            Edit Goal
          </button>
          <button
            type="button"
            className={buttonClass}
            disabled={busy}
            onClick={onInterruptTurn}
          >
            Stop current turn
          </button>
          <button
            type="button"
            className={buttonClass}
            disabled={busy}
            onClick={() => {
              if (window.confirm('Clear this Goal permanently?')) onClear();
            }}
          >
            Clear Goal
          </button>
        </div>
      )}
      {!isRunActive &&
        goal.status !== AgentGoalStatus.complete &&
        onResumeSaved && (
          <div className="mt-base space-y-base">
            <button
              type="button"
              className={buttonClass}
              disabled={busy || resumeSavedDisabled}
              onClick={onResumeSaved}
            >
              Resume Goal
            </button>
            <p className="text-xs text-low">
              Continue the saved Goal in this Codex session without replacing
              its objective or resetting usage. Codex checks whether it still
              exists.
            </p>
          </div>
        )}
      {error && <p className="mt-base text-xs text-error">{error}</p>}
    </section>
  );
}
