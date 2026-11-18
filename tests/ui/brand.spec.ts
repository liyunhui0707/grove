import { test, expect } from '@playwright/test';
test('Grove has one consistent visual identity and generates the public comparison illustration', async ({
  page,
}, testInfo) => {
  await page.goto('/');
  await expect(page).toHaveTitle('Grove');
  await expect(page.locator('.brand')).toContainText('Grove');
  await expect(page.locator('.brand img')).toHaveAttribute('src', '/grove.svg');
  await expect(page.locator('.brand')).not.toContainText('Moment Grove');
  await page.getByRole('button', { name: '体验示例图库', exact: true }).click();
  await expect(page.locator('.photo-card').first()).toBeVisible();
  await page.locator('.select-photo').nth(0).click();
  await page.locator('.select-photo').nth(1).click();
  await page.keyboard.press('c');
  await expect(page.locator('.review-workspace')).toBeVisible();
  await expect(page.locator('.photo-pane')).toHaveCount(2);
  await page.screenshot({ path: testInfo.outputPath('grove-comparison.png') });
});
