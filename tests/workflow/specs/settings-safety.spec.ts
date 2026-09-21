import { expect, test, type Page } from '@playwright/test';

function inventory(provider: string, label: string) {
  return {
    providers: [
      {
        provider,
        installed: true,
        provider_version: 'fixture',
        executable_path: label,
        schema_revision: 'schema',
        capabilities: {
          native_writable: true,
          profile_storage: true,
          per_run_overrides: true,
          raw_editable: true,
        },
        descriptors: [],
        effective_settings: [],
        withheld_setting_keys: [],
        unknown_native_nodes: [],
        limitations: [],
        errors: [],
        native_files: [
          {
            id: 'config',
            path: `/fixture/${provider}`,
            format: 'toml',
            scope: 'user',
            exists: true,
            parse_status: 'parsed',
            revision: 'native-revision',
            writable: true,
            raw_editable: true,
          },
        ],
      },
    ],
    errors: [],
  };
}

async function setup(page: Page) {
  await page.route('**/api/**', async (route) => {
    const url = new URL(route.request().url());
    if (url.pathname === '/api/agent-settings') {
      const provider = url.searchParams.get('provider') ?? 'codex';
      return route.fulfill({
        json: {
          success: true,
          data: inventory(provider, `${provider}-executable`),
        },
      });
    }
    if (url.pathname === '/api/agent-settings/profiles')
      return route.fulfill({ json: { success: true, data: [] } });
    if (url.pathname === '/api/profiles')
      return route.fulfill({
        json: {
          success: true,
          data: {
            content: '{"executors":{}}',
            path: '/fixture/profiles',
            revision: 'profile-revision',
            sensitive_values_included:
              url.searchParams.get('confirmed_sensitive_read') === 'true',
          },
        },
      });
    // Optional remote discovery failure must not disable explicit local editing.
    return route.fulfill({
      status: 503,
      json: { success: false, message: 'fixture remote unavailable' },
    });
  });
}

test('unknown explicit Host does not fall back; optional remote failure keeps local usable', async ({
  page,
}) => {
  await setup(page);
  let settingsRequests = 0;
  page.on('request', (request) => {
    if (new URL(request.url()).pathname === '/api/agent-settings')
      settingsRequests++;
  });
  await page.goto('/?mode=settings-safety&host=missing-host');
  await expect(page.getByTestId('host-policy')).toContainText(
    'missing-host:false'
  );
  expect(settingsRequests).toBe(0);
  await page.getByRole('button', { name: 'Local Host', exact: true }).click();
  await expect(page.getByTestId('host-policy')).toContainText('local:true');
  await expect(
    page.getByText('codex-executable', { exact: true })
  ).toBeVisible();
  await expect(page.getByTestId('host-policy')).toHaveText('local:true:true');
});

test('late provider discovery cannot overwrite another provider panel', async ({
  page,
}) => {
  await setup(page);
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  let started!: () => void;
  const ready = new Promise<void>((resolve) => {
    started = resolve;
  });
  await page.route('**/api/agent-settings?provider=codex', async (route) => {
    started();
    await gate;
    await route.fulfill({
      json: { success: true, data: inventory('codex', 'late-codex') },
    });
  });
  await page.goto('/?mode=settings-safety');
  await ready;
  await page.getByRole('button', { name: 'Claude provider' }).click();
  await expect(
    page.getByText('claude_code-executable', { exact: true })
  ).toBeVisible();
  release();
  await expect(page.getByText('late-codex', { exact: true })).toHaveCount(0);
  await expect(
    page.getByText('claude_code-executable', { exact: true })
  ).toBeVisible();
});

test('full profile read is explicit and save carries the observed revision', async ({
  page,
}) => {
  await setup(page);
  const reads: string[] = [];
  let written: unknown;
  await page.route('**/api/profiles**', async (route) => {
    if (route.request().method() === 'PUT') {
      written = route.request().postDataJSON();
      return route.fulfill({
        status: 409,
        json: { success: false, message: 'external edit' },
      });
    }
    const url = new URL(route.request().url());
    reads.push(url.searchParams.get('confirmed_sensitive_read') ?? 'false');
    return route.fallback();
  });
  await page.goto('/?mode=settings-safety');
  await expect.poll(() => reads).toEqual(['false']);
  await expect(
    page.getByRole('button', { name: 'Save profile fixture' })
  ).toBeDisabled();
  await page.getByRole('button', { name: 'Explicit profile read' }).click();
  await expect(page.getByTestId('profile-consent')).toHaveText('true');
  expect(reads).toEqual(['false', 'true']);
  await page.getByRole('button', { name: 'Save profile fixture' }).click();
  await expect(page.getByTestId('profile-save')).toHaveText('conflict');
  expect(written).toEqual({
    content: '{"executors":{}}',
    expected_revision: 'profile-revision',
    confirmed_sensitive_read: true,
  });
  await page.getByRole('button', { name: 'Unknown Host' }).click();
  await expect(page.getByTestId('profile-consent')).toHaveText('false');
  await expect(
    page.getByRole('button', { name: 'Save profile fixture' })
  ).toBeDisabled();
});
