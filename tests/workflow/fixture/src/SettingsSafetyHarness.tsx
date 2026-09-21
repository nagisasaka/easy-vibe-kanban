import { useState } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { AppRuntimeProvider } from '@/shared/hooks/useAppRuntime';
import { AuthContext } from '@/shared/hooks/auth/useAuth';
import {
  SettingsHostProvider,
  useSettingsHost,
  useSettingsMachineClient,
} from '@/shared/dialogs/settings/settings/SettingsHostContext';
import { SettingsDirtyProvider } from '@/shared/dialogs/settings/settings/SettingsDirtyContext';
import { AgentConfigurationSettingsPanel } from '@/shared/dialogs/settings/settings/AgentConfigurationSettingsPanel';
import { useMachineProfiles } from '@/shared/hooks/useProfiles';
import { BaseCodingAgent } from 'shared/types';

const client = new QueryClient({
  defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
});

function Content() {
  const host = useSettingsHost();
  const machine = useSettingsMachineClient();
  const profiles = useMachineProfiles(machine);
  const [provider, setProvider] = useState(BaseCodingAgent.CODEX);
  const [saved, setSaved] = useState('');
  return (
    <main>
      <output data-testid="host-policy">
        {host.selectedHostId}:{String(host.canEdit)}:
        {String(host.discoveryFailed)}
      </output>
      <button onClick={() => host.setSelectedHostId('missing-host')}>
        Unknown Host
      </button>
      <button onClick={() => host.setSelectedHostId('local')}>
        Local Host
      </button>
      <button onClick={() => setProvider(BaseCodingAgent.CLAUDE_CODE)}>
        Claude provider
      </button>
      <button onClick={() => setProvider(BaseCodingAgent.CODEX)}>
        Codex provider
      </button>
      <output data-testid="profile-consent">
        {String(profiles.hasSensitiveValues)}
      </output>
      <button onClick={profiles.reveal}>Explicit profile read</button>
      <button
        disabled={!profiles.hasSensitiveValues || !host.canEdit}
        onClick={() =>
          void profiles
            .save(profiles.profilesContent)
            .then(() => setSaved('saved'))
            .catch(() => setSaved('conflict'))
        }
      >
        Save profile fixture
      </button>
      <output data-testid="profile-save">{saved}</output>
      <SettingsDirtyProvider>
        <AgentConfigurationSettingsPanel executor={provider} variant={null} />
      </SettingsDirtyProvider>
    </main>
  );
}

export function SettingsSafetyHarness() {
  const initialHost =
    new URLSearchParams(window.location.search).get('host') ?? 'local';
  return (
    <QueryClientProvider client={client}>
      <AppRuntimeProvider runtime="local">
        <AuthContext.Provider
          value={{ isSignedIn: false, isLoaded: true, userId: null }}
        >
          <SettingsHostProvider initialHostId={initialHost}>
            <Content />
          </SettingsHostProvider>
        </AuthContext.Provider>
      </AppRuntimeProvider>
    </QueryClientProvider>
  );
}
