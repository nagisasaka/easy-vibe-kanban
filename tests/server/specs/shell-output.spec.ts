import { expect, test } from '@playwright/test';

test('development shell displays its command and streams output while expanded', async ({
  page,
}) => {
  const sourceRoot = process.env.LVK_E2E_SOURCE_ROOT;
  test.skip(
    !sourceRoot,
    'Requires the development Vite server and its checkout path'
  );
  await page.goto('/workspaces/create', { waitUntil: 'load' });
  await expect(page.getByRole('button').first()).toBeVisible({
    timeout: 120_000,
  });
  // An empty development database may open the workspace picker on arrival.
  await page.keyboard.press('Escape');
  // Use the real development modules with deterministic events, without starting
  // a paid agent or modifying the shared application's database.
  await page.evaluate(async (path) => {
    const fixture = await import(/* @vite-ignore */ path);
    fixture.mount();
  }, `/@fs${sourceRoot}/tests/server/fixtures/shell-output.tsx`);
  const shell = page.getByRole('region', { name: 'Shell regression' });
  const command = shell.getByRole('button', {
    name: 'printf first; sleep 1; printf second',
  });
  await expect(command).toBeVisible();
  await command.click();
  await expect(command).toHaveAttribute('aria-expanded', 'true');
  const output = shell.locator('pre').last();
  await expect(output).toHaveText('');
  await shell.getByRole('button', { name: 'First output' }).click();
  await expect(output).toHaveText('first\n');
  await shell.getByRole('button', { name: 'Second output' }).click();
  await expect(output).toHaveText('first\nsecond\n');
  await shell.getByRole('button', { name: 'Complete shell' }).click();
  await expect(output).toHaveText('first\nsecond\n');
  await expect(command).toHaveAttribute('aria-expanded', 'true');
});
