import { test, expect, type Page } from '@playwright/test';
async function setup(page: Page) {
  await page.route('http://grove.localhost/**', (route) =>
    route.fulfill({
      contentType: 'image/svg+xml',
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="48" height="32"><path fill="#617b68" d="M0 0h48v32H0z"/></svg>',
    }),
  );
  await page.addInitScript(() => {
    const assets = Array.from({ length: 6 }, (_, i) => ({
      id: `photo-${i}`,
      folderId: 1,
      name: `photo-${i}.jpg`,
      relative: `photo-${i}.jpg`,
      width: 4032,
      height: 3024,
      bytes: 500000,
      modified: i,
      captured: null,
      rating: null,
      revision: 0,
      format: 'jpg',
    }));
    const seen: string[] = [];
    const aborted: string[] = [];
    const fetchOriginal = window.fetch.bind(window);
    Object.assign(window, {
      qaRequests: seen,
      qaAborted: aborted,
      qaHold: false,
      isTauri: true,
      __TAURI_INTERNALS__: {
        transformCallback: () => 1,
        unregisterCallback: () => {},
        convertFileSrc: (id: string) => `http://grove.localhost/${id}`,
        invoke: async (cmd: string, args: Record<string, any> = {}) => {
          if (
            [
              'register_style_exit_guard',
              'record_startup_stage',
              'save_workspace',
              'cancel_scan',
            ].includes(cmd)
          )
            return;
          if (cmd === 'load_workspace') return null;
          if (cmd === 'choose_folder') return { id: 1, name: 'Generated', demo: false };
          if (cmd === 'scan_folder') {
            args.onEvent.onmessage({ kind: 'batch', items: assets });
            args.onEvent.onmessage({ kind: 'done', count: 6, warnings: [] });
            return;
          }
          if (cmd === 'set_rating') {
            localStorage.setItem('rated', JSON.stringify(args.ids));
            return {
              token: 'rated',
              changes: args.ids.map((id: string) => ({
                id,
                before: null,
                after: args.rating,
                revision: 1,
              })),
            };
          }
          throw Error(cmd);
        },
      },
    });
    window.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      seen.push(url);
      if ((window as any).qaHold && url.includes('/photo-1?size=1600'))
        return new Promise((_resolve, reject) => {
          init?.signal?.addEventListener(
            'abort',
            () => {
              aborted.push(url);
              reject(new DOMException('Aborted', 'AbortError'));
            },
            { once: true },
          );
        });
      return fetchOriginal(input, init);
    }) as typeof fetch;
  });
  await page.goto('/');
  await page
    .locator('.welcome')
    .getByRole('button', { name: '打开照片文件夹', exact: true })
    .click();
  await expect(page.getByRole('heading', { name: '所有照片 6' })).toBeVisible();
}
const previews = (page: Page) =>
  page.evaluate(
    () => (window as any).qaRequests.filter((s: string) => s.includes('size=1600')) as string[],
  );
test('single review prepares only the next screen-sized preview and ratings retain the active target', async ({
  page,
}) => {
  await setup(page);
  expect(await previews(page)).toEqual([]);
  await page.getByRole('button', { name: '查看 photo-0.jpg', exact: true }).dblclick();
  await expect(page.locator('.photo-pane img')).toHaveAttribute('alt', 'photo-0.jpg');
  await expect
    .poll(() => previews(page))
    .toContain('http://grove.localhost/photo-1?size=1600&stamp=1');
  expect((await previews(page)).some((s) => s.includes('photo-2'))).toBe(false);
  await page.getByRole('button', { name: '下一张', exact: true }).click();
  await expect(page.locator('.photo-pane img')).toHaveAttribute('alt', 'photo-1.jpg');
  await page.getByRole('checkbox', { name: '评分后下一张' }).uncheck();
  await page.locator('.pane-canvas').click();
  await page.keyboard.press('5');
  await expect.poll(() => page.evaluate(() => localStorage.getItem('rated'))).toBe('["photo-1"]');
  await expect
    .poll(() => previews(page))
    .toContain('http://grove.localhost/photo-2?size=1600&stamp=2');
});
test('rapid navigation cancels stale preparation and the final photo does not wrap or prepare detail originals', async ({
  page,
}) => {
  await setup(page);
  await page.evaluate(() => {
    (window as any).qaHold = true;
  });
  await page.getByRole('button', { name: '查看 photo-0.jpg', exact: true }).dblclick();
  await expect
    .poll(() => previews(page))
    .toContain('http://grove.localhost/photo-1?size=1600&stamp=1');
  await page.getByRole('button', { name: '下一张', exact: true }).click();
  await page.getByRole('button', { name: '下一张', exact: true }).click();
  await expect(page.locator('.photo-pane img')).toHaveAttribute('alt', 'photo-2.jpg');
  await expect.poll(() => page.evaluate(() => (window as any).qaAborted.length)).toBeGreaterThan(0);
  const firstCount = (await previews(page)).filter((s) => s.includes('photo-0')).length;
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: '查看 photo-5.jpg', exact: true }).dblclick();
  await expect(page.locator('.photo-pane img')).toHaveAttribute('alt', 'photo-5.jpg');
  await page.waitForTimeout(400);
  const urls = await previews(page);
  expect(urls.filter((s) => s.includes('photo-0'))).toHaveLength(firstCount);
  expect(
    await page.evaluate(() => (window as any).qaRequests.some((s: string) => s.includes('8192'))),
  ).toBe(false);
});
