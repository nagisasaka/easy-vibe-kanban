import { expect, test } from '@playwright/test';

test('unretrieved and failed diff trees never claim there are no changes', async ({
  page,
}) => {
  await page.goto('/?mode=diff-tree');
  await expect(page.getByRole('status')).toBeVisible();
  await expect(page.getByText('No changed files')).toHaveCount(0);
  await page.getByRole('button', { name: 'Disconnect' }).click();
  await expect(page.getByRole('alert')).toContainText('stream unavailable');
  await expect(page.getByText('No changed files')).toHaveCount(0);
  await page.getByRole('button', { name: 'Empty snapshot' }).click();
  await expect(page.getByRole('alert')).toHaveCount(0);
  await expect(page.getByText('No changed files')).toBeVisible();
  await page.getByRole('button', { name: 'Disconnect' }).click();
  await expect(page.getByRole('alert')).toContainText('last loaded');
  await expect(page.getByText('No changed files')).toHaveCount(0);
});

test('a failed refresh labels cached files and clears them on workspace switch', async ({
  page,
}) => {
  await page.goto('/?mode=diff-tree');
  await page.getByRole('button', { name: 'Changed snapshot' }).click();
  await expect(page.getByText('labels.mjs', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Disconnect' }).click();
  await expect(page.getByRole('alert')).toContainText('last loaded');
  await expect(page.getByText('labels.mjs', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Switch workspace' }).click();
  await expect(page.getByText('labels.mjs', { exact: true })).toHaveCount(0);
  await expect(page.getByRole('status')).toBeVisible();
  await expect(page.getByText('No changed files')).toHaveCount(0);
});
