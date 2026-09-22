import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Undo2, Redo2 } from 'lucide-react';
import { Button } from '@vibe/ui/components/Button';

export function WorkflowHistoryControls({
  canUndo,
  canRedo,
  onMove,
}: {
  canUndo: boolean;
  canRedo: boolean;
  onMove: (direction: 'undo' | 'redo') => void;
}) {
  const { t } = useTranslation('common');
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (
        event.defaultPrevented ||
        event.altKey ||
        event.isComposing ||
        !(event.ctrlKey || event.metaKey)
      )
        return;
      const target = event.target;
      if (
        target instanceof HTMLElement &&
        target.closest(
          'input,textarea,select,[contenteditable="true"],[role="dialog"]'
        )
      )
        return;
      const key = event.key.toLowerCase();
      const direction =
        key === 'z'
          ? event.shiftKey
            ? 'redo'
            : 'undo'
          : key === 'y'
            ? 'redo'
            : null;
      if (!direction || !(direction === 'undo' ? canUndo : canRedo)) return;
      event.preventDefault();
      onMove(direction);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [canUndo, canRedo, onMove]);
  return (
    <>
      <Button
        variant="outline"
        disabled={!canUndo}
        onClick={() => onMove('undo')}
        aria-label={t('workflow.draft.undo')}
        title={t('workflow.draft.undo')}
      >
        <Undo2 className="h-4 w-4" />
      </Button>
      <Button
        variant="outline"
        disabled={!canRedo}
        onClick={() => onMove('redo')}
        aria-label={t('workflow.draft.redo')}
        title={t('workflow.draft.redo')}
      >
        <Redo2 className="h-4 w-4" />
      </Button>
    </>
  );
}
