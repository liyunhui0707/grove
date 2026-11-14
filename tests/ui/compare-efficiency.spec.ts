import { test, expect } from '@playwright/test';

// Fixed task: keep photo 01 on the left, inspect/rate photos 02–08 on the right.
// Counts completed clicks/keystrokes, not assertions or automation wall time.
for (const variant of ['grid-baseline', 'continuous'] as const)
  test(`two-pane comparison task: ${variant}`, async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: '体验示例图库' }).click();
    const actions: string[] = [];
    const act = async (name: string, run: () => Promise<unknown>) => {
      await run();
      actions.push(name);
    };
    const name = (n: number) => `山海_${String(n).padStart(2, '0')}.jpg`;
    const select = (n: number) =>
      page.getByRole('button', { name: `选择 ${name(n)}`, exact: true });
    const pane = (n: number) =>
      page.getByRole('region', { name: `比较照片 ${name(n)}`, exact: true });
    await act('select reference', () => select(1).click());
    await act('select candidate 2', () => select(2).click());
    await act('enter comparison', () => page.keyboard.press('c'));
    await act('activate candidate 2', () => pane(2).click());
    const ratings = [0, 1, 2, 3, 4, 5, 0];
    for (let n = 2; n <= 8; n++) {
      if (n > 2 && variant === 'continuous') {
        await act(`next candidate ${n}`, () => page.keyboard.press(']'));
      } else if (n > 2) {
        await act('return to grid', () => page.keyboard.press('Escape'));
        await act(`deselect ${n - 1}`, () => select(n - 1).click());
        await act(`select candidate ${n}`, () => select(n).click());
        await act('enter comparison', () => page.keyboard.press('c'));
        await act(`activate candidate ${n}`, () => pane(n).click());
      }
      await expect(page.locator('.photo-pane')).toHaveCount(2);
      await expect(page.locator('.photo-pane').first().locator('.pane-name')).toContainText(
        name(1),
      );
      const rating = ratings[n - 2];
      await act(`rate candidate ${n}=${rating}`, () => page.keyboard.press(String(rating)));
      await expect(
        pane(n).getByLabel(rating ? `${rating} 星` : '0 分：淘汰候选', { exact: true }),
      ).toHaveClass(/rated/);
      await expect(pane(1).getByLabel('1 星', { exact: true })).not.toHaveClass(/rated/);
    }
    await page.keyboard.press('Escape');
    for (let n = 1; n <= 8; n++) {
      const card = page
        .locator('.photo-card')
        .filter({ has: page.getByLabel(`查看 ${name(n)}`, { exact: true }) });
      await expect(card.locator('.rating-badge')).toHaveText(
        n === 1 ? '未评分' : ratings[n - 2] === 0 ? '淘汰候选' : String(ratings[n - 2]),
      );
    }
    expect(actions).toHaveLength(variant === 'continuous' ? 17 : 41);
    console.log(
      `COMPARE_EFFICIENCY ${JSON.stringify({ variant, candidates: 7, panes: 2, actions: actions.length, candidateSwitchActions: variant === 'continuous' ? 6 : 30, ratings, referenceUnreviewed: true, log: actions })}`,
    );
  });
