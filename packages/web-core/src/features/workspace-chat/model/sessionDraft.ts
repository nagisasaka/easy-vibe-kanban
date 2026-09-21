/** A send acknowledgement may consume only the exact composer it submitted. */
export interface SessionDraftSubmission {
  readonly scope: string;
  readonly revision: number;
  readonly content: string;
}

export function isSessionDraftSubmissionCurrent(
  current: SessionDraftSubmission,
  submission: SessionDraftSubmission
): boolean {
  return (
    current.scope === submission.scope &&
    current.revision === submission.revision &&
    current.content === submission.content
  );
}
