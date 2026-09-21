import { useEffect, useRef, useState } from 'react';
import { useBlocker } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { Button } from '@vibe/ui/components/Button';
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from '@vibe/ui/components/Dialog';

export function WorkflowDraftGuard({
  dirty,
  saving,
  canSave,
  onSave,
  onDiscard,
  hasPendingChanges,
  errorMessage,
}: {
  dirty: boolean;
  saving: boolean;
  canSave: boolean;
  onSave: () => Promise<boolean>;
  onDiscard: () => boolean;
  hasPendingChanges: () => boolean;
  errorMessage?: string | null;
}) {
  const { t } = useTranslation('common');
  const returnFocus = useRef<HTMLElement | null>(null);
  const blocker = useBlocker({
    shouldBlockFn: () => {
      if (!hasPendingChanges()) return false;
      returnFocus.current = document.activeElement as HTMLElement;
      return true;
    },
    enableBeforeUnload: dirty || saving,
    withResolver: true,
  });
  const [savingToLeave, setSavingToLeave] = useState(false);
  const [saveFailed, setSaveFailed] = useState(false);
  const blocked = blocker.status === 'blocked';
  useEffect(() => {
    if (!blocked) setSaveFailed(false);
  }, [blocked]);
  return (
    <Dialog
      open={blocked}
      onOpenChange={(open) => {
        if (!open && !savingToLeave) blocker.reset?.();
      }}
    >
      <DialogContent
        className="new-design p-double"
        hideCloseButton
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          returnFocus.current?.focus();
        }}
      >
        <DialogTitle>{t('workflow.draft.leaveTitle')}</DialogTitle>
        <DialogDescription>
          {t('workflow.draft.leaveDescription')}
        </DialogDescription>
        {saveFailed && (
          <p role="alert">{errorMessage || t('workflow.draft.saveFailed')}</p>
        )}
        <DialogFooter>
          <Button
            variant="outline"
            disabled={savingToLeave}
            onClick={() => blocker.reset?.()}
          >
            {t('workflow.draft.keepEditing')}
          </Button>
          <Button
            variant="outline"
            disabled={saving || savingToLeave}
            onClick={() => {
              if (onDiscard()) blocker.proceed?.();
            }}
          >
            {t('workflow.draft.discardLeave')}
          </Button>
          <Button
            disabled={!canSave || saving || savingToLeave}
            onClick={async () => {
              setSavingToLeave(true);
              setSaveFailed(false);
              try {
                if (await onSave()) blocker.proceed?.();
                else setSaveFailed(true);
              } catch {
                setSaveFailed(true);
              } finally {
                setSavingToLeave(false);
              }
            }}
          >
            {t('workflow.draft.saveLeave')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
