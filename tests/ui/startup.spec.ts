import { test, expect } from '@playwright/test';

for (const theme of ['light', 'dark'] as const) {
  test(`a missing entry module gives a private, usable ${theme} recovery screen`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 960, height: 640 });
    await page.emulateMedia({ colorScheme: theme });
    await page.addInitScript(() => localStorage.setItem('startup-sentinel', 'keep-local-data'));
    let blocked = true;
    // Vite hoists the production module and drops custom entry attributes.
    await page.route('/', async (route) => {
      const response = await route.fetch();
      await route.fulfill({
        response,
        body: (await response.text()).replace('data-startup-entry', ''),
      });
    });
    await page.route('**/apps/desktop/src/main.tsx', (route) =>
      blocked ? route.abort() : route.continue(),
    );
    await page.goto('/');
    const startup = page.getByRole('region', { name: 'Grove 启动状态' });
    await expect(startup).toContainText('MG-START-MODULE');
    const retry = startup.getByRole('button', { name: '重新载入界面', exact: true });
    await expect(retry).toBeInViewport({ ratio: 1 });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
      true,
    );
    await page.screenshot({ path: `.local/startup-stage/entry-${theme}.png` });
    blocked = false;
    await retry.click();
    await expect(page.getByRole('button', { name: '体验示例图库', exact: true })).toBeVisible();
    await expect(startup).toBeHidden();
    expect(await page.evaluate(() => localStorage.getItem('startup-sentinel'))).toBe(
      'keep-local-data',
    );
  });
}

test('initial render failure shows a fixed code without exception text', async ({ page }) => {
  await page.route('**/apps/desktop/src/App.tsx', (route) =>
    route.fulfill({
      contentType: 'text/javascript',
      body: 'export function App(){throw new Error("/private/generated/photo-name.jpg sensitive-test-value");}',
    }),
  );
  await page.goto('/');
  const startup = page.getByRole('region', { name: 'Grove 启动状态' });
  await expect(startup).toContainText('MG-START-RENDER');
  await expect(startup).not.toContainText('photo-name');
  await expect(startup).not.toContainText('sensitive-test-value');
  await expect(startup.getByRole('button', { name: '重新载入界面' })).toBeEnabled();
});

test('a slow initial render can become ready without an automatic reload', async ({ page }) => {
  await page.clock.install();
  let release!: () => void;
  const pending = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route('**/apps/desktop/src/App.tsx', async (route) => {
    await pending;
    await route.continue();
  });
  let navigations = 0;
  page.on('framenavigated', (frame) => {
    if (frame === page.mainFrame()) navigations++;
  });
  await page.goto('/', { waitUntil: 'domcontentloaded' });
  await page.clock.fastForward(16000);
  const startup = page.getByRole('region', { name: 'Grove 启动状态' });
  await expect(startup).toContainText('MG-START-WAIT');
  release();
  await expect(page.getByRole('button', { name: '体验示例图库', exact: true })).toBeVisible();
  await expect(startup).toBeHidden();
  await page.evaluate(() => (window as any).__momentStartup.fail('module'));
  await expect(startup).toBeHidden();
  expect(navigations).toBe(1);
});
