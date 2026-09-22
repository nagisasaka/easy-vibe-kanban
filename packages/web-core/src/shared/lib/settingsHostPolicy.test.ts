import { describe, expect, it } from 'vitest';
import { canEditSettingsHost, initialSettingsHost } from './settingsHostPolicy';

const local = { id: 'local', kind: 'local' as const };
const online = {
  id: 'remote-a',
  kind: 'remote' as const,
  status: 'online' as const,
};

describe('settings Host identity', () => {
  it('retains unknown explicit and route identities instead of falling back', () => {
    expect(initialSettingsHost([local, online], 'local', null, 'missing')).toBe(
      'missing'
    );
    expect(initialSettingsHost([local], 'local', 'missing')).toBe('missing');
    expect(initialSettingsHost([], 'remote', 'remote-a')).toBe('remote-a');
    expect(
      initialSettingsHost([local, online], 'local', 'remote-a', 'local')
    ).toBe('local');
  });
  it('requires a known online remote Host, without blocking unrelated local settings', () => {
    expect(canEditSettingsHost(null, true, false)).toBe(false);
    expect(canEditSettingsHost(online, false, false)).toBe(false);
    expect(canEditSettingsHost(online, true, true)).toBe(false);
    expect(
      canEditSettingsHost({ ...online, status: 'offline' }, true, false)
    ).toBe(false);
    expect(canEditSettingsHost(online, true, false)).toBe(true);
    expect(canEditSettingsHost(local, true, true)).toBe(true);
  });
});
