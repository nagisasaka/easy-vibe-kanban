import { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import type { ErrorComponentProps } from '@tanstack/react-router';
import { CrashScreen } from '@vibe/ui/components/CrashScreen';

/** Shared by local and remote routing, not by the chat/Wiki view toggle. */
export function RoutePending() {
  const { t } = useTranslation('common');
  return (
    <div
      className="new-design bg-primary p-double text-normal"
      role="status"
      aria-busy="true"
    >
      {t('states.loading')}
    </div>
  );
}

export function RouteLoadError({ error }: ErrorComponentProps) {
  const focusRef = useRef<HTMLDivElement>(null);
  useEffect(() => focusRef.current?.focus(), []);
  return (
    <div className="new-design" role="alert" tabIndex={-1} ref={focusRef}>
      <CrashScreen error={error} />
    </div>
  );
}

export const routeLoadOptions = {
  defaultPendingComponent: RoutePending,
  defaultErrorComponent: RouteLoadError,
  defaultPendingMs: 150,
};
