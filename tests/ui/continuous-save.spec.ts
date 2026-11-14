import { test, expect } from '@playwright/test';

test('pending native ratings block replacement, and late replies affect only their target', async ({
  page,
}) => {
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
          if (command === 'load_workspace') return null;
          if (command === 'save_workspace' || command === 'cancel_scan') return;
          if (command === 'choose_folder') return { id: 1, name: 'Owned test', demo: false };
          if (command === 'scan_folder') {
            args.onEvent.onmessage({ kind: 'batch', items: assets });
            args.onEvent.onmessage({ kind: 'done', count: 4, warnings: [] });
            return;
          }
          if (command === 'set_rating') {
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
  for (const id of [0, 1])
    await page.getByRole('button', { name: `选择 photo-${id}.jpg`, exact: true }).click();
  await page.keyboard.press('c');
  const candidate = page.getByRole('region', { name: '比较照片 photo-1.jpg', exact: true });
  await candidate.click();
  await page.keyboard.press('4');
  await expect(page.getByRole('button', { name: '下一候选', exact: true })).toBeDisabled();
  await page.keyboard.press(']');
  await expect(candidate).toHaveClass(/active-pane/);
  expect(await page.evaluate(() => (window as any).__ratingTarget)).toEqual(['photo-1']);
  await page.evaluate(() => (window as any).__releaseRating());
  await expect(candidate.getByLabel('4 星', { exact: true })).toHaveClass(/rated/);
  await page.keyboard.press(']');
  await expect(page.getByRole('region', { name: '比较照片 photo-2.jpg', exact: true })).toHaveClass(
    /active-pane/,
  );
  await expect(
    page
      .getByRole('region', { name: '比较照片 photo-0.jpg', exact: true })
      .getByLabel('1 星', { exact: true }),
  ).not.toHaveClass(/rated/);
  await page.keyboard.press('[');
  await expect(candidate.getByLabel('4 星', { exact: true })).toHaveClass(/rated/);
});
