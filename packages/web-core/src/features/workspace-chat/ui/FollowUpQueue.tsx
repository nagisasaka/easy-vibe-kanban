import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { QueuedMessage } from 'shared/types';
import WYSIWYGEditor from '@/shared/components/WYSIWYGEditor';
import { queueApi } from '@/shared/lib/api';

type Props = {
  messages: QueuedMessage[];
  paused: boolean;
  busy: boolean;
  running: boolean;
  error?: string;
  onEdit: (operation: Parameters<typeof queueApi.edit>[1]) => Promise<void>;
  onResume: () => Promise<void>;
};

export function FollowUpQueue({
  messages,
  paused,
  busy,
  running,
  error,
  onEdit,
  onResume,
}: Props) {
  const { t } = useTranslation('tasks');
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState('');
  const act = (operation: Parameters<typeof queueApi.edit>[1]) => {
    void onEdit(operation).catch(() => {});
  };
  const move = (index: number, offset: number) => {
    const ids = messages.map((message) => message.id);
    [ids[index], ids[index + offset]] = [ids[index + offset], ids[index]];
    act({ type: 'reorder', ids });
  };
  if (!messages.length && !error) return null;
  return (
    <section
      aria-label="Follow-up queue"
      className="mb-2 rounded border p-3 text-sm"
    >
      <div className="mb-2 flex items-center justify-between gap-2">
        <strong>
          {t('followUpQueue.title', { count: messages.length })}
          {paused ? ` · ${t('followUpQueue.paused')}` : ''}
        </strong>
        {!running && messages.length > 0 && (
          <button
            type="button"
            disabled={busy}
            onClick={() => {
              void onResume().catch(() => {});
            }}
          >
            {t('followUpQueue.resume')}
          </button>
        )}
      </div>
      <p className="mb-2 whitespace-normal break-words text-xs text-low">
        {t('followUpQueue.description')}
      </p>
      <ol className="flex max-h-64 flex-col gap-2 overflow-y-auto">
        {messages.map((message, index) => (
          <li
            key={message.id}
            className="rounded border p-2"
            data-queue-id={message.id}
          >
            {editing === message.id ? (
              <>
                <textarea
                  aria-label={t('followUpQueue.content')}
                  className="w-full rounded border bg-transparent p-2"
                  value={draft}
                  onChange={(event) => setDraft(event.target.value)}
                />
                <button
                  type="button"
                  className="mr-3"
                  disabled={busy || !draft.trim()}
                  onClick={() => {
                    void onEdit({
                      type: 'edit',
                      id: message.id,
                      message: draft,
                    })
                      .then(() => setEditing(null))
                      .catch(() => {});
                  }}
                >
                  {t('followUpQueue.save')}
                </button>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => setEditing(null)}
                >
                  {t('followUpQueue.cancelEdit')}
                </button>
              </>
            ) : (
              <>
                <div className="flex gap-2">
                  <span>{index + 1}.</span>
                  <div className="min-w-0 flex-1">
                    <WYSIWYGEditor
                      value={message.data.message}
                      disabled
                      className="text-sm"
                    />
                  </div>
                </div>
                <div className="mt-1 flex gap-3">
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => {
                      setEditing(message.id);
                      setDraft(message.data.message);
                    }}
                  >
                    {t('followUpQueue.edit')}
                  </button>
                  <button
                    type="button"
                    aria-label={t('followUpQueue.moveUp', {
                      position: index + 1,
                    })}
                    disabled={busy || index === 0}
                    onClick={() => move(index, -1)}
                  >
                    ↑
                  </button>
                  <button
                    type="button"
                    aria-label={t('followUpQueue.moveDown', {
                      position: index + 1,
                    })}
                    disabled={busy || index === messages.length - 1}
                    onClick={() => move(index, 1)}
                  >
                    ↓
                  </button>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => act({ type: 'remove', id: message.id })}
                  >
                    {t('followUpQueue.remove')}
                  </button>
                </div>
              </>
            )}
          </li>
        ))}
      </ol>
      {error && (
        <p role="alert" className="mt-2 text-error">
          {error}
        </p>
      )}
    </section>
  );
}
