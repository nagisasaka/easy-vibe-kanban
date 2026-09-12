import { useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@vibe/ui/components/KeyboardDialog';
import {
  buildWikiBootstrapPrompt,
  prepareWikiDraft,
} from '../model/wikiBootstrap';

export function WikiBootstrapDialog({
  workspaceId,
  repository,
  initialLanguage,
  onClose,
}: {
  workspaceId: string;
  repository: string;
  initialLanguage: string;
  onClose: () => void;
}) {
  const [language, setLanguage] = useState(initialLanguage);
  const [instructions, setInstructions] = useState('');
  const [replace, setReplace] = useState(false);
  const [occupied, setOccupied] = useState(false);
  const [message, setMessage] = useState('');
  const [prepared, setPrepared] = useState(false);
  const submit = () => {
    const code = language.trim();
    if (
      !/^(?:[a-z]{2,8}|x)(?:-[a-z0-9]{1,8})*$/i.test(code) ||
      code.length > 63 ||
      code.toLowerCase() === 'x'
    ) {
      setMessage('Enter a language tag such as ja, en, or pt-BR.');
      return;
    }
    const result = prepareWikiDraft(
      workspaceId,
      buildWikiBootstrapPrompt({ repository, language: code, instructions }),
      replace
    );
    if (result === 'occupied') {
      setOccupied(true);
      setMessage(
        'The chat has an unsent draft. Confirm replacement to continue.'
      );
    } else if (result === 'unavailable') {
      setMessage(
        'Open an idle chat session in this workspace first. Finish approval, edit, review, attachment, or queued-message actions before preparing a Wiki request.'
      );
    } else {
      setPrepared(true);
      setMessage(
        result === 'goal'
          ? 'The complete request is in the chat with Goal mode selected. Review it and send when ready.'
          : 'The complete request is in the chat. This agent has no supported Goal integration; send it as a normal message or select Codex and Goal mode.'
      );
    }
  };
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create Wiki from existing code</DialogTitle>
          <DialogDescription>
            Prepare an editable chat request for {repository}. Codex selects
            Goal mode automatically. Nothing is sent or written until you send
            the request.
          </DialogDescription>
        </DialogHeader>
        {!prepared && (
          <div className="space-y-base">
            <label className="block">
              Output language
              <input
                className="mt-half w-full rounded border bg-primary p-half"
                value={language}
                onChange={(e) => setLanguage(e.target.value)}
                list="wiki-bootstrap-languages"
              />
              <datalist id="wiki-bootstrap-languages">
                <option value="ja">日本語</option>
                <option value="en">English</option>
                <option value="de">Deutsch</option>
                <option value="fr">Français</option>
                <option value="es">Español</option>
                <option value="ko">한국어</option>
                <option value="zh-Hans">简体中文</option>
                <option value="pt-BR">Português</option>
              </datalist>
            </label>
            <label className="block">
              Investigation preferences (optional)
              <textarea
                className="mt-half w-full rounded border bg-primary p-half"
                rows={4}
                value={instructions}
                onChange={(e) => setInstructions(e.target.value)}
                placeholder="Areas to focus on or exclude"
              />
            </label>
            {occupied && (
              <label className="flex gap-half">
                <input
                  type="checkbox"
                  checked={replace}
                  onChange={(e) => setReplace(e.target.checked)}
                />
                Replace the current unsent text draft
              </label>
            )}
          </div>
        )}
        {message && <p role="status">{message}</p>}
        <div className="flex justify-end gap-base">
          <button
            type="button"
            onClick={onClose}
            className="rounded border px-base py-half"
          >
            {prepared ? 'Return to chat' : 'Cancel'}
          </button>
          {!prepared && (
            <button
              type="button"
              onClick={submit}
              disabled={occupied && !replace}
              className="rounded bg-brand px-base py-half text-white disabled:opacity-50"
            >
              Set in chat
            </button>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
