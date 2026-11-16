import { test, expect, type Page } from '@playwright/test';

async function pendingFolder(page: Page) {
  await page.addInitScript(() => {
    const assets = Array.from({ length: 4 }, (_, i) => ({
      id: `photo-${i}`,
      folderId: 1,
      name: `photo-${i}.jpg`,
      relative: `photo-${i}.jpg`,
      width: 64,
      height: 48,
      bytes: 100,
      modified: i,
      captured: null,
      rating: null,
      revision: 0,
      format: 'jpg',
    }));
    Object.assign(window, {
      isTauri: true,
      __TAURI_INTERNALS__: {
        transformCallback: () => 1,
        unregisterCallback: () => {},
        convertFileSrc: (id: string) => `http://grove-test.invalid/${id}`,
        invoke: async (command: string, args: Record<string, any> = {}) => {
          if (command === 'register_style_exit_guard') return;
          if (command === 'load_workspace') return null;
          if (command === 'save_workspace' || command === 'cancel_scan') return;
          if (command === 'choose_folder') return { id: 1, name: 'Owned test', demo: false };
          if (command === 'scan_folder') {
            args.onEvent.onmessage({ kind: 'batch', items: assets });
            args.onEvent.onmessage({ kind: 'done', count: 4, warnings: [] });
            return;
          }
          if (command === 'set_rating') {
            ((window as any).__ratingCalls ??= []).push({ ids: args.ids, rating: args.rating });
            (window as any).__ratingTarget = args.ids;
            await new Promise<void>((resolve) => {
              (window as any).__releaseRating = resolve;
            });
            return {
              token: 'late-rating',
              changes: args.ids.map((id: string) => ({
                id,
                before: null,
                after: args.rating,
                revision: 1,
              })),
            };
          }
          if (command === 'undo_rating')
            return {
              token: 'undo',
              changes: (window as any).__ratingTarget.map((id: string) => ({
                id,
                before: 4,
                after: null,
                revision: 2,
              })),
            };
          throw Error(`Unhandled ${command}`);
        },
      },
    });
  });
  await page.route('http://grove-test.invalid/**', (route) =>
    route.fulfill({
      contentType: 'image/svg+xml',
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="48"><path fill="#617b68" d="M0 0h64v48H0z"/></svg>',
    }),
  );
  await page.goto('/');
  await page
    .locator('.welcome')
    .getByRole('button', { name: '打开照片文件夹', exact: true })
    .click();
}

test('pending comparison rating allows next candidate but never retargets the write', async ({
  page,
}) => {
  await pendingFolder(page);
  for (const id of [0, 1])
    await page.getByRole('button', { name: `选择 photo-${id}.jpg`, exact: true }).click();
  await page.keyboard.press('c');
  const candidate = page.getByRole('region', { name: '比较照片 photo-1.jpg', exact: true });
  await candidate.click();
  await page.keyboard.press('4');
  await expect(page.getByRole('button', { name: '下一候选', exact: true })).toBeEnabled();
  // The reply is deliberately unresolved: navigation overlaps the save wait.
  await page.getByRole('button', { name: '下一候选', exact: true }).click();
  const next = page.getByRole('region', { name: '比较照片 photo-2.jpg', exact: true });
  await expect(next).toHaveClass(/active-pane/);
  await page.keyboard.press('5');
  await expect(page.getByRole('status').filter({ hasText: '刚才的评分未执行' })).toBeVisible();
  expect(await page.evaluate(() => (window as any).__ratingCalls)).toEqual([
    { ids: ['photo-1'], rating: 4 },
  ]);
  await page.evaluate(() => (window as any).__releaseRating());
  await expect(page.locator('.save-indicator')).toHaveCount(0);
  await expect(next.getByLabel('5 星', { exact: true })).not.toHaveClass(/rated/);
  await expect(page.getByRole('status').filter({ hasText: '刚才的评分未执行' })).toBeVisible();
  await page.keyboard.press('[');
  await expect(candidate.getByLabel('4 星', { exact: true })).toHaveClass(/rated/);
  await page.keyboard.press('Control+z');
  await expect(candidate.getByLabel('4 星', { exact: true })).not.toHaveClass(/rated/);
  await expect(candidate).toHaveClass(/active-pane/);
  await expect(
    page
      .getByRole('region', { name: '比较照片 photo-0.jpg', exact: true })
      .getByLabel('1 星', { exact: true }),
  ).not.toHaveClass(/rated/);
});

test('single review navigates during save without a late extra advance or held-key ratings', async ({
  page,
}) => {
  await pendingFolder(page);
  await page.getByRole('button', { name: '查看 photo-0.jpg', exact: true }).dblclick();
  await page.keyboard.press('4');
  await page.getByRole('button', { name: '下一张', exact: true }).click();
  await expect(
    page.getByRole('region', { name: '比较照片 photo-1.jpg', exact: true }),
  ).toBeVisible();
  await page.keyboard.press('Control+z');
  await expect(page.getByRole('status').filter({ hasText: '刚才的撤销未执行' })).toBeVisible();
  await page.evaluate(() => (window as any).__releaseRating());
  await expect(page.locator('.save-indicator')).toHaveCount(0);
  await expect(
    page.getByRole('region', { name: '比较照片 photo-1.jpg', exact: true }),
  ).toBeVisible();
  await page.keyboard.down('5');
  await page.evaluate(() => (window as any).__releaseRating());
  await expect(
    page.getByRole('region', { name: '比较照片 photo-2.jpg', exact: true }),
  ).toBeVisible();
  await page.keyboard.down('5'); // OS repeat after the first save already finished.
  await page.keyboard.up('5');
  expect(await page.evaluate(() => (window as any).__ratingCalls)).toEqual([
    { ids: ['photo-0'], rating: 4 },
    { ids: ['photo-1'], rating: 5 },
  ]);
  await expect(page.getByRole('status').filter({ hasText: '刚才的撤销未执行' })).toHaveCount(0);
});

for (const theme of ['light', 'dark']) {
  test(`busy input feedback fits a small ${theme} window and respects changed auto-advance`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 960, height: 640 });
    await pendingFolder(page);
    if (theme === 'dark') await page.getByLabel('切换暗色主题').click();
    await page.getByRole('button', { name: '查看 photo-0.jpg', exact: true }).dblclick();
    await page.keyboard.press('4');
    await page.keyboard.press('5');
    const feedback = page.getByRole('status').filter({ hasText: '刚才的评分未执行' });
    await expect(feedback).toBeVisible();
    expect(
      await feedback.evaluate((el) => {
        const r = el.getBoundingClientRect();
        return (
          r.left >= 0 &&
          r.right <= innerWidth &&
          r.bottom <= innerHeight &&
          el.scrollWidth <= el.clientWidth
        );
      }),
    ).toBe(true);
    await page.getByLabel('评分后下一张', { exact: true }).uncheck();
    await page.screenshot({ path: `test-results/busy-feedback-${theme}.png` });
    await page.evaluate(() => (window as any).__releaseRating());
    await expect(page.locator('.save-indicator')).toHaveCount(0);
    await expect(
      page.getByRole('region', { name: '比较照片 photo-0.jpg', exact: true }),
    ).toBeVisible();
    await expect(feedback).toBeVisible();
    expect(await page.evaluate(() => (window as any).__ratingCalls)).toHaveLength(1);
  });
}
