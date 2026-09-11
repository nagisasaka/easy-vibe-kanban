import { useEffect, useState } from 'react';

interface PlanGoalApprovalCardProps {
  objective: string;
  busy?: boolean;
  error?: string | null;
  onApprove: (objective: string) => void;
}

export function PlanGoalApprovalCard({
  objective,
  busy = false,
  error,
  onApprove,
}: PlanGoalApprovalCardProps) {
  const [draft, setDraft] = useState(objective);

  useEffect(() => setDraft(objective), [objective]);

  return (
    <section className="border-border bg-primary mb-base rounded-md border p-base">
      <div className="space-y-half">
        <h3 className="text-sm font-medium text-high">
          Plan with Goal — review objective
        </h3>
        <p className="text-xs text-low">
          Edit the complete Goal objective below. The persistent Goal starts
          only after you approve it.
        </p>
      </div>
      <textarea
        value={draft}
        onChange={(event) => setDraft(event.currentTarget.value)}
        rows={8}
        className="border-border bg-secondary text-normal mt-base w-full resize-y rounded-sm border p-base text-sm"
      />
      <div className="mt-base flex justify-end">
        <button
          type="button"
          disabled={busy || !draft.trim()}
          onClick={() => onApprove(draft.trim())}
          className="bg-brand text-on-brand hover:bg-brand-hover rounded-sm px-double py-base text-sm disabled:cursor-not-allowed disabled:opacity-50"
        >
          {busy ? 'Starting Goal…' : 'Approve and Start Goal'}
        </button>
      </div>
      {error && <p className="mt-base text-xs text-error">{error}</p>}
    </section>
  );
}
