import { test, expect, type Page } from '@playwright/test';

async function installLibrary(page: Page) {
  await page.route('http://grove.localhost/**', (route) =>
    route.fulfill({
      contentType: 'image/svg+xml',
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="320" height="240"><path fill="#617b68" d="M0 0h320v240H0z"/></svg>',
    }),
  );
  await page.addInitScript(() => {
    const assets = Array.from({ length: 3000 }, (_, i) => ({
      id: `photo-${i}`,
      folderId: 1,
      name: `照片_${String(i).padStart(5, '0')}.jpg`,
      relative: `照片_${String(i).padStart(5, '0')}.jpg`,
      width: 320,
      height: 240,
      bytes: 10000,
      modified: i,
      captured: null,
      rating: JSON.parse(localStorage.getItem(`rating-${i}`) || 'null'),
      revision: 0,
      format: 'jpg',
    }));
    Object.assign(window, {
      isTauri: true,
      __TAURI_INTERNALS__: {
        transformCallback: () => 1,
        unregisterCallback: () => {},
        convertFileSrc: () => 'http://grove.localhost/generated-photo',
        invoke: async (cmd: string, args: Record<string, any> = {}) => {
          if (cmd === 'register_style_exit_guard' || cmd === 'record_startup_stage') return;
          if (cmd === 'export_history') return [];
          if (cmd === 'load_workspace')
            return JSON.parse(localStorage.getItem('workspace') || 'null');
          if (cmd === 'resume_workspace' || cmd === 'choose_folder')
            return { id: 1, name: 'Generated 3000', demo: false };
          if (cmd === 'save_workspace') {
            localStorage.setItem(
              'workspace',
              JSON.stringify({ folderId: 1, name: 'Generated 3000', view: args.view }),
            );
            return;
          }
          if (cmd === 'scan_folder') {
            for (let i = 0; i < assets.length; i += 64) {
              args.onEvent.onmessage({ kind: 'batch', items: assets.slice(i, i + 64) });
              if (localStorage.getItem('delayed-scan'))
                await new Promise((resolve) => setTimeout(resolve, 10));
            }
            args.onEvent.onmessage({ kind: 'done', count: 3000, warnings: [] });
            return;
          }
          if (cmd === 'cancel_scan') return;
          if (cmd === 'set_rating') {
            localStorage.setItem('rated-ids', JSON.stringify(args.ids));
            return {
              token: 'large-rate',
              changes: args.ids.map((id: string) => {
                const a = assets.find((a) => a.id === id)!;
                const before = a.rating;
                a.rating = args.rating;
                a.revision++;
                localStorage.setItem(`rating-${id.split('-')[1]}`, JSON.stringify(a.rating));
                return { id, before, after: a.rating, revision: a.revision };
              }),
            };
          }
          throw Error(`Unhandled ${cmd}`);
        },
      },
    });
  });
}

const name = (n: number) => `照片_${String(n).padStart(5, '0')}.jpg`;
const openPhoto = (page: Page, n: number) =>
  page.getByRole('button', { name: `查看 ${name(n)}`, exact: true });
const choosePhoto = (page: Page, n: number) =>
  page.getByRole('button', { name: `选择 ${name(n)}`, exact: true });
async function begin(page: Page) {
  await installLibrary(page);
  await page.goto('/');
  await page
    .locator('.welcome')
    .getByRole('button', { name: '打开照片文件夹', exact: true })
    .click();
  await expect(page.getByRole('button', { name: '所有照片 3000', exact: true })).toBeVisible();
}
async function walk(page: Page, n: number) {
  await openPhoto(page, 0).dblclick();
  for (let i = 0; i < n; i++) await page.keyboard.press('ArrowRight');
  await expect(page.locator('.photo-pane')).toContainText(name(n));
}
async function visibleTogether(page: Page, indices: number[]) {
  const viewport = await page.getByLabel('照片网格').boundingBox();
  if (!viewport) return false;
  for (const n of indices) {
    if (!(await openPhoto(page, n).count())) return false;
    const box = await openPhoto(page, n).boundingBox();
    if (!box || box.y < viewport.y || box.y + box.height > viewport.y + viewport.height)
      return false;
  }
  return true;
}

// Small windows may have a grid shorter than a user-sized thumbnail.
// Require the maximum possible visible height, rather than changing density.
async function maximallyVisible(page: Page, n: number) {
  if (!(await openPhoto(page, n).count())) return false;
  const box = await openPhoto(page, n).boundingBox();
  const viewport = await page.getByLabel('照片网格').boundingBox();
  if (!box || !viewport) return false;
  const shown = Math.max(
    0,
    Math.min(box.y + box.height, viewport.y + viewport.height) - Math.max(box.y, viewport.y),
  );
  return (
    shown >= Math.min(box.height, viewport.height) - 2 &&
    box.x >= viewport.x &&
    box.x + box.width <= viewport.x + viewport.width + 1
  );
}

test('return task locates the last reviewed occurrence before comparison and picks export', async ({
  page,
}) => {
  await begin(page);
  await walk(page, 40);
  await page.getByRole('checkbox', { name: '评分后下一张' }).uncheck();
  await page.locator('.pane-name').click();
  await page.keyboard.press('4');
  await expect(page.getByText('审片位置已保存', { exact: true })).toBeVisible();
  await page.reload();
  await expect(page.locator('.photo-pane')).toContainText(name(40));
  await page.keyboard.press('Escape');
  const baseline = process.env.MG_RECORD_BASELINE === '1';
  let scrolls = 0;
  if (baseline) {
    const grid = page.getByLabel('照片网格');
    const box = (await grid.boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    while (!(await visibleTogether(page, [40, 41])) && scrolls < 20) {
      const before = await grid.evaluate((el) => el.scrollTop);
      await page.mouse.wheel(0, 600);
      await expect.poll(() => grid.evaluate((el) => el.scrollTop)).not.toBe(before);
      scrolls++;
    }
    expect(scrolls).toBeGreaterThan(0);
  }
  await expect.poll(() => visibleTogether(page, [40, 41])).toBe(true);
  await expect(page.locator('.photo-card.focused')).toContainText(name(40));
  await expect(page.getByText('审片位置已保存', { exact: true })).toBeVisible();
  await page.reload();
  await expect.poll(() => visibleTogether(page, [40, 41])).toBe(true);
  expect(await page.locator('.photo-card').count()).toBeLessThan(60);
  await openPhoto(page, 40).click();
  await choosePhoto(page, 41).click();
  await page.keyboard.press('c');
  await expect(page.locator('.photo-pane')).toHaveCount(2);
  await page.getByRole('region', { name: `比较照片 ${name(41)}`, exact: true }).click();
  await page.keyboard.press('5');
  await expect(page.locator('.photo-pane.active-pane .pane-name')).toContainText(name(41));
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: '精选照片 2', exact: true }).click();
  await page.getByRole('button', { name: '导出照片副本', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '导出照片副本' });
  await expect(dialog).toContainText(name(40));
  await expect(dialog).toContainText(name(41));
  await expect(dialog).toContainText('2 张');
  const ratings = await page.evaluate(() => [
    localStorage.getItem('rating-40'),
    localStorage.getItem('rating-41'),
    localStorage.getItem('rating-0'),
  ]);
  expect(ratings).toEqual(['4', '5', null]);
  await page.screenshot({
    path: `.local/daily-efficiency-stage/${baseline ? 'baseline' : 'improved'}-export.png`,
  });
  console.log(
    `DAILY_EFFICIENCY ${JSON.stringify({ variant: baseline ? 'baseline' : 'improved', assets: 3000, reviewedIndex: 40, commonNavigationKeys: 40, locateScrollGestures600px: scrolls, comparisonRatings: [4, 5], unrelatedUnreviewed: true, exportPlanPhotos: 2 })}`,
  );
});

test('return button reveals the current identity after a narrow dark layout change without selecting it', async ({
  page,
}) => {
  await begin(page);
  await walk(page, 27);
  await page.setViewportSize({ width: 960, height: 640 });
  await page.getByRole('button', { name: '切换暗色主题' }).click();
  await page.getByRole('button', { name: '返回照片', exact: true }).click();
  await expect.poll(() => maximallyVisible(page, 27)).toBe(true);
  await expect(choosePhoto(page, 27)).toHaveAttribute('aria-pressed', 'false');
  await expect(openPhoto(page, 27).locator('img')).toHaveJSProperty('naturalWidth', 320);
  await page.screenshot({ path: '.local/daily-efficiency-stage/return-dark-small.png' });
  const grid = page.getByLabel('照片网格');
  await grid.evaluate((el) => {
    el.scrollTop = 0;
  });
  await expect.poll(() => maximallyVisible(page, 0)).toBe(true);
  // A later rating update must not pull a user who scrolled away back to the old target.
  await openPhoto(page, 0).click();
  await page.keyboard.press('2');
  await expect.poll(() => maximallyVisible(page, 0)).toBe(true);
});

test('return never clears a filter to reveal a photo that just left it', async ({ page }) => {
  await begin(page);
  await page.getByRole('button', { name: '未评分 3000', exact: true }).click();
  await walk(page, 0);
  await page.getByRole('checkbox', { name: '评分后下一张' }).uncheck();
  await page.locator('.pane-name').click();
  await page.keyboard.press('0');
  await expect(page.getByText('审片位置已保存', { exact: true })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByLabel('评分筛选')).toHaveValue('unreviewed');
  await expect(openPhoto(page, 0)).toHaveCount(0);
  await expect(openPhoto(page, 1)).toBeInViewport({ ratio: 1 });
  expect(await page.evaluate(() => localStorage.getItem('rated-ids'))).toBe('["photo-0"]');
});

test('comparison return reveals the replaced active pane and keeps the exact selection', async ({
  page,
}) => {
  await begin(page);
  await choosePhoto(page, 0).click();
  await choosePhoto(page, 1).click();
  await page.keyboard.press('c');
  await page.getByRole('region', { name: `比较照片 ${name(1)}`, exact: true }).click();
  for (let i = 0; i < 40; i++) await page.keyboard.press(']');
  await expect(page.locator('.photo-pane.active-pane')).toContainText(name(41));
  await page.keyboard.press('Escape');
  await expect(openPhoto(page, 41)).toBeInViewport({ ratio: 1 });
  await expect(choosePhoto(page, 41)).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('.statusbar')).toContainText('已选择 2 张');
});

test('grid resume waits for streamed scan and measured columns before restoring a clamped offset', async ({
  page,
}) => {
  await installLibrary(page);
  await page.addInitScript(() => {
    localStorage.setItem('delayed-scan', 'true');
    // Hold the final spacer layout for two frames, as if the last streamed
    // batch's size had not reached layout when the restore command runs.
    const observer = new MutationObserver((records) => {
      const complete = [...document.querySelectorAll('button')].some(
        (button) => button.textContent?.replace(/\s/g, '') === '所有照片3000',
      );
      if (!complete) return;
      for (const record of records) {
        const spacer = record.target as HTMLElement;
        if (!spacer.parentElement?.classList.contains('gallery-viewport')) continue;
        const finalHeight = spacer.style.height;
        if (!(parseFloat(finalHeight) > 1000)) continue;
        observer.disconnect();
        (window as any).__heldGridLayout = true;
        const overflow = spacer.style.overflow;
        spacer.style.height = `${parseFloat(finalHeight) * 0.97}px`;
        spacer.style.overflow = 'hidden';
        requestAnimationFrame(() =>
          requestAnimationFrame(() => {
            spacer.style.height = finalHeight;
            spacer.style.overflow = overflow;
          }),
        );
        break;
      }
    });
    observer.observe(document, {
      subtree: true,
      attributes: true,
      attributeFilter: ['style'],
    });
    if (!localStorage.getItem('workspace'))
      localStorage.setItem(
        'workspace',
        JSON.stringify({
          folderId: 1,
          name: 'Generated 3000',
          view: {
            mode: 'grid',
            active: 'photo-2999',
            filter: 'all',
            sort: 'name',
            search: '',
            size: 200,
            autoAdvance: true,
            theme: 'light',
            offset: 1000000,
          },
        }),
      );
  });
  await page.setViewportSize({ width: 1720, height: 1050 });
  await page.goto('/');
  await expect(page.getByRole('button', { name: '所有照片 3000', exact: true })).toBeVisible();
  try {
    await expect.poll(() => maximallyVisible(page, 2999)).toBe(true);
  } catch (error) {
    const geometry = await page.getByLabel('照片网格').evaluate((grid) => {
      const card = grid.querySelector('[aria-label="查看 照片_02999.jpg"]');
      return {
        top: grid.scrollTop,
        height: grid.scrollHeight,
        viewport: grid.clientHeight,
        spacer: (grid.firstElementChild as HTMLElement)?.style.height,
        grid: grid.getBoundingClientRect().toJSON(),
        card: card?.getBoundingClientRect().toJSON(),
        workspace: JSON.parse(localStorage.getItem('workspace') || 'null'),
      };
    });
    await test.info().attach('grid-resume-geometry', {
      body: JSON.stringify(geometry),
      contentType: 'application/json',
    });
    throw error;
  }
  expect(await page.evaluate(() => (window as any).__heldGridLayout)).toBe(true);
  const grid = page.getByLabel('照片网格');
  await expect.poll(() => grid.evaluate((el) => el.scrollTop)).toBeGreaterThan(1000);
  await expect
    .poll(() => grid.evaluate((el) => Math.abs(el.scrollHeight - el.clientHeight - el.scrollTop)))
    .toBeLessThanOrEqual(1);
  await grid.evaluate((el) => {
    el.scrollTop = 0;
  });
  await expect.poll(() => grid.evaluate((el) => el.scrollTop)).toBe(0);
  await expect
    .poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('workspace')!).view.offset))
    .toBe(0);
  await page.reload();
  await expect(page.getByRole('button', { name: '所有照片 3000', exact: true })).toBeVisible();
  await expect(openPhoto(page, 0)).toBeInViewport();
  await expect.poll(() => grid.evaluate((el) => el.scrollTop)).toBe(0);
});

test('streamed grid resume keeps saved browsing offset instead of forcing the active photo', async ({
  page,
}) => {
  await installLibrary(page);
  await page.addInitScript(() => {
    localStorage.setItem('delayed-scan', 'true');
    localStorage.setItem(
      'workspace',
      JSON.stringify({
        folderId: 1,
        name: 'Generated 3000',
        view: {
          mode: 'grid',
          active: 'photo-2999',
          filter: 'all',
          sort: 'name',
          search: '',
          size: 260,
          autoAdvance: false,
          theme: 'dark',
          offset: 5000,
        },
      }),
    );
  });
  await page.goto('/');
  await expect(page.getByRole('button', { name: '所有照片 3000', exact: true })).toBeVisible();
  await expect.poll(() => page.getByLabel('照片网格').evaluate((el) => el.scrollTop)).toBe(5000);
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  expect(await openPhoto(page, 2999).count()).toBe(0);
  expect(await page.evaluate(() => localStorage.getItem('rated-ids'))).toBeNull();
});

test('an empty restored search does not leave a scroll request for a later filter change', async ({
  page,
}) => {
  await installLibrary(page);
  await page.addInitScript(() => {
    localStorage.setItem('delayed-scan', 'true');
    localStorage.setItem(
      'workspace',
      JSON.stringify({
        folderId: 1,
        name: 'Generated 3000',
        view: {
          mode: 'grid',
          active: 'photo-2999',
          filter: 'all',
          sort: 'name',
          search: 'no-match',
          size: 200,
          autoAdvance: true,
          theme: 'light',
          offset: 5000,
        },
      }),
    );
  });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: '没有符合条件的照片' })).toBeVisible();
  await page.getByRole('textbox', { name: '搜索照片' }).fill('');
  await expect(page.getByRole('button', { name: '所有照片 3000', exact: true })).toBeVisible();
  await expect(openPhoto(page, 0)).toBeInViewport();
  await expect.poll(() => page.getByLabel('照片网格').evaluate((el) => el.scrollTop)).toBe(0);
});

test('sidebar reflow keeps the visible selection and remembers its setting', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await begin(page);
  const grid = page.getByLabel('照片网格');
  const expanded = (await grid.boundingBox())!;
  expect(expanded.y).toBeLessThan(90);
  const columns = () => page.locator('.photo-row').first().locator('.photo-card').count();
  const before = await columns();
  await grid.evaluate((el) => {
    el.scrollTop = 40000;
  });
  const target = grid.locator('.photo-open').filter({ visible: true });
  await expect
    .poll(async () => await target.first().getAttribute('aria-label'))
    .not.toBe('查看 照片_00000.jpg');
  const inView = await grid.evaluate((el) => {
    const bounds = el.getBoundingClientRect();
    return Array.from(el.querySelectorAll<HTMLButtonElement>('.photo-open'))
      .find((button) => {
        const rect = button.getBoundingClientRect();
        return rect.top >= bounds.top && rect.bottom <= bounds.bottom;
      })
      ?.getAttribute('aria-label');
  });
  expect(inView).toBeTruthy();
  const chosen = page.getByRole('button', { name: inView!, exact: true });
  await chosen.click();
  await page.getByRole('button', { name: '隐藏侧栏' }).press('Enter');
  await expect(page.getByRole('button', { name: '显示侧栏' })).toHaveAttribute(
    'aria-expanded',
    'false',
  );
  await expect.poll(columns).toBeGreaterThan(before);
  await expect(chosen).toBeInViewport({ ratio: 0.98 });
  await expect(chosen.locator('..')).toHaveClass(/selected/);
  await page.reload();
  await expect(page.getByRole('button', { name: '显示侧栏' })).toBeVisible();
  await expect(page.getByRole('navigation', { name: '资料库' })).toBeHidden();
  await page.getByRole('button', { name: '显示侧栏' }).click();
  await expect(page.getByRole('navigation', { name: '资料库' })).toBeVisible();
});
