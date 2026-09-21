import { expect, test } from '@playwright/test';

test('lazy deep link reports loading and works after reload', async ({
  page,
}) => {
  await page.goto('/lazy?mode=route-loading');
  await expect(page.getByRole('status')).toBeVisible();
  await expect(page.getByText('Lazy route content')).toBeVisible();
  await page.reload();
  await expect(page.getByText('Lazy route content')).toBeVisible();
});

test('lazy failure is visible and keyboard reload can recover', async ({
  page,
}) => {
  await page.goto('/lazy?mode=route-loading&fail=1');
  await expect(page.getByRole('alert')).toBeVisible();
  await expect(page.getByRole('alert')).toBeFocused();
  await expect(page.getByText('Lazy route content')).toHaveCount(0);
  await page.evaluate(() =>
    history.replaceState(null, '', '/lazy?mode=route-loading')
  );
  await page.keyboard.press('Tab');
  await expect(
    page.getByRole('button', { name: 'Reload', exact: true })
  ).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.getByText('Lazy route content')).toBeVisible();
  await expect(page.getByRole('alert')).toHaveCount(0);
});
