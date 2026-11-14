import { test, expect, type Page } from '@playwright/test';

const name = (n: number) => `山海_${String(n).padStart(2, '0')}.jpg`;
const pane = (page: Page, n: number) =>
  page.getByRole('region', { name: `比较照片 ${name(n)}`, exact: true });
async function compare(page: Page, ids = [1, 2]) {
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  for (const id of ids)
    await page.getByRole('button', { name: `选择 ${name(id)}`, exact: true }).click();
  await page.keyboard.press('c');
}

test('replacement preserves the group order and skips every visible photo', async ({ page }) => {
  await compare(page, [1, 2, 3]);
  await expect(page.getByRole('button', { name: '上一候选', exact: true })).toBeDisabled();
  await pane(page, 3).click();
  await page.keyboard.press(']');
  await expect(page.locator('.photo-pane .pane-name')).toHaveText([
    /山海_01/,
    /山海_02/,
    /山海_04/,
  ]);
  await page.keyboard.press('[');
  await expect(pane(page, 3)).toHaveClass(/active-pane/);
  await pane(page, 1).click();
  await page.getByRole('button', { name: '下一候选', exact: true }).click();
  await expect(page.locator('.photo-pane .pane-name')).toHaveText([
    /山海_04/,
    /山海_02/,
    /山海_03/,
  ]);
  await page.keyboard.press('5');
  await expect(pane(page, 4).getByLabel('5 星', { exact: true })).toHaveClass(/rated/);
  await expect(pane(page, 2).getByLabel('1 星', { exact: true })).not.toHaveClass(/rated/);
  await page.keyboard.press('Escape');
  for (const id of [2, 3, 4])
    await expect(
      page.getByRole('button', { name: `选择 ${name(id)}`, exact: true }),
    ).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('button', { name: `选择 ${name(1)}`, exact: true })).toHaveAttribute(
    'aria-pressed',
    'false',
  );
});

test('a frozen filtered queue and undo retain exact rating targets', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  await page.getByRole('button', { name: /^未评分/ }).click();
  await page.getByLabel('排序方式').selectOption('rating');
  for (const id of [1, 2])
    await page.getByRole('button', { name: `选择 ${name(id)}`, exact: true }).click();
  await page.keyboard.press('c');
  await pane(page, 2).click();
  await page.keyboard.press('4');
  await expect(pane(page, 2).getByLabel('4 星', { exact: true })).toHaveClass(/rated/);
  await page.keyboard.press(']');
  await expect(pane(page, 3)).toHaveClass(/active-pane/);
  await page.keyboard.press('0');
  await expect(pane(page, 3).getByLabel('0 分：淘汰候选')).toHaveClass(/rated/);
  await page.keyboard.press(']');
  await expect(pane(page, 4)).toHaveClass(/active-pane/);
  await page.keyboard.press('Control+z');
  await expect(pane(page, 3)).toHaveClass(/active-pane/);
  await expect(pane(page, 3).getByLabel('0 分：淘汰候选')).not.toHaveClass(/rated/);
  await page.keyboard.press('Control+z');
  await expect(pane(page, 2)).toHaveClass(/active-pane/);
  await expect(pane(page, 2).getByLabel('1 星', { exact: true })).not.toHaveClass(/rated/);
  await expect(pane(page, 1).getByLabel('1 星', { exact: true })).not.toHaveClass(/rated/);
});

test('candidate bounds respect the captured search, and controls fit both small themes', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 640 });
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  await page.getByLabel('搜索照片').fill('山海_0');
  for (const id of [1, 2])
    await page.getByRole('button', { name: `选择 ${name(id)}`, exact: true }).click();
  await page.keyboard.press('c');
  await pane(page, 2).click();
  for (let id = 3; id <= 9; id++) {
    await page.keyboard.press(']');
    await expect(pane(page, id)).toHaveClass(/active-pane/);
  }
  await expect(page.getByRole('button', { name: '下一候选', exact: true })).toBeDisabled();
  await page.keyboard.press(']');
  await expect(pane(page, 9)).toHaveClass(/active-pane/);
  await page.keyboard.press('Space');
  await expect(page.locator('.photo-pane')).toHaveCount(1);
  await expect(page.getByRole('button', { name: '上一候选', exact: true })).toBeDisabled();
  await page.keyboard.press('[');
  await expect(pane(page, 9)).toHaveClass(/active-pane/);
  await page.keyboard.press('Escape');
  await expect(page.locator('.photo-pane')).toHaveCount(2);
  const controls = page.getByRole('group', { name: '连续比较' });
  expect(
    await controls.evaluate((el) => {
      const r = el.getBoundingClientRect();
      return r.left >= 0 && r.right <= innerWidth && r.bottom <= innerHeight;
    }),
  ).toBe(true);
  await page.screenshot({ path: 'test-results/continuous-compare-small-light.png' });
  await page.getByLabel('切换暗色主题').click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await expect(page.locator('.app-shell')).toHaveCSS('background-color', 'rgb(28, 33, 30)');
  await page.screenshot({ path: 'test-results/continuous-compare-small-dark.png' });
  await page.keyboard.press('Escape');
  await page.getByLabel('搜索照片').fill('山海_0');
  await page.keyboard.press(']');
  await expect(page.getByLabel('搜索照片')).toHaveValue('山海_0]');
});
