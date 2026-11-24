import { test, expect, type Page } from '@playwright/test';

async function scene(page: Page) {
  await page.route('http://grove-qa.invalid/**', (route) =>
    route.fulfill({
      contentType: 'image/svg+xml',
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="48" height="32"><path fill="#617b68" d="M0 0h48v32H0z"/></svg>',
    }),
  );
  await page.addInitScript(() => {
    const callbacks = new Map<number, (event: any) => void>();
    let callbackId = 0,
      menuHandler = 0;
    const qa = {
      calls: [] as { cmd: string; args: any }[],
      exit: (_token: number) => {},
      failOpen: false,
      holdExit: false,
      releaseExit: () => {},
      command: (command: string) => callbacks.get(menuHandler)?.({ payload: command }),
    };
    const assets = Array.from({ length: 4 }, (_, i) => ({
      id: `photo-${i}`,
      folderId: 1,
      name: `photo-${i}.jpg`,
      relative: `photo-${i}.jpg`,
      width: 48,
      height: 32,
      bytes: 100,
      modified: i,
      captured: null,
      rating: i === 0 ? 0 : null,
      revision: 0,
      format: 'jpg',
    }));
    let report: any = null;
    Object.assign(window, {
      isTauri: true,
      qa,
      __TAURI_INTERNALS__: {
        transformCallback: (callback: (event: any) => void) => {
          callbacks.set(++callbackId, callback);
          return callbackId;
        },
        unregisterCallback: (id: number) => callbacks.delete(id),
        convertFileSrc: (id: string) => `http://grove-qa.invalid/${id}`,
        invoke: async (cmd: string, args: any = {}) => {
          qa.calls.push({ cmd, args });
          if (cmd === 'plugin:event|listen') {
            menuHandler = args.handler;
            return 1;
          }
          if (
            cmd === 'plugin:event|unlisten' ||
            cmd === 'cancel_scan' ||
            cmd === 'record_startup_stage' ||
            cmd === 'save_watermark_style' ||
            cmd === 'resolve_style_exit'
          )
            return;
          if (cmd === 'register_style_exit_guard') {
            qa.exit = (token) => args.onRequest.onmessage(token);
            return;
          }
          if (cmd === 'load_workspace' || cmd === 'load_watermark_style') return null;
          if (cmd === 'save_workspace') {
            if (qa.holdExit) await new Promise<void>((resolve) => (qa.releaseExit = resolve));
            return;
          }
          if (cmd === 'choose_folder' || cmd === 'create_demo') {
            if (qa.failOpen) throw Error('EACCES generated-private-path');
            return { id: 1, name: 'Generated', demo: false };
          }
          if (cmd === 'scan_folder') {
            args.onEvent.onmessage({ kind: 'batch', items: assets });
            args.onEvent.onmessage({ kind: 'done', count: assets.length, warnings: [] });
            return;
          }
          if (cmd === 'set_rating')
            return {
              token: 'rating',
              changes: args.ids.map((id: string) => {
                const asset = assets.find((a) => a.id === id)!;
                const before = asset.rating;
                asset.rating = args.rating;
                return { id, before, after: args.rating, revision: ++asset.revision };
              }),
            };
          if (cmd === 'export_history') return report?.status === 'completed' ? [report] : [];
          if (cmd === 'choose_export_folder') {
            report = {
              id: 'owned-job',
              kind: args.watermark ? 'watermark' : 'copy',
              status: 'ready',
              destination: '/generated/output/Long folder name 中文',
              verified: 0,
              bytes: 200,
              watermark: args.watermark,
              items: args.targets.map((t: any) => ({
                name: assets.find((a) => a.id === t.id)!.name,
                output: '001-photo.jpg',
                motionOutput: args.watermark ? null : '001-photo.mov',
                rating: null,
                bytes: 100,
                status: 'pending',
                error: null,
              })),
            };
            return report;
          }
          if (cmd === 'confirm_export') {
            report = {
              ...report,
              status: 'completed',
              verified: report.items.length,
              items: report.items.map((a: any) => ({ ...a, status: 'verified' })),
            };
            args.onEvent.onmessage(report);
            return report;
          }
          if (cmd === 'open_export_folder') return;
          if (cmd === 'watermark_preview')
            return {
              canExport: true,
              dataUrl: 'http://grove-qa.invalid/preview',
              exportWidth: 48,
              exportHeight: 32,
              info: {
                source: {
                  dateSource: 'exif',
                  date: '2026-11-01',
                  text: '2026-11-01',
                  width: 48,
                  height: 32,
                  warning: null,
                },
              },
            };
          if (cmd === 'move_history' || cmd === 'list_relocation_folders') return [];
          if (cmd === 'list_backups') return { items: [], unavailable: 0 };
          if (cmd === 'inspect_xmp')
            return { pending: 0, recoveries: 0, supported: true, offset: 0, items: [] };
          if (cmd === 'application_info')
            return {
              version: '0.8.0',
              buildId: 'qa-owned',
              isolated: true,
              dataDirectory: '/generated/runtime',
              executable: '/generated/Grove.app',
            };
          throw Error(cmd);
        },
      },
    });
  });
  await page.goto('/');
  await page
    .locator('.welcome')
    .getByRole('button', { name: '打开照片文件夹', exact: true })
    .click();
  await expect(page.locator('.photo-card')).toHaveCount(4);
}
const tools = (page: Page) => page.getByRole('button', { name: '资料库工具', exact: true });

test('hidden selection is explicit, inspectable and consistent across compare, rating and export', async ({
  page,
}) => {
  await scene(page);
  await page.getByRole('button', { name: '选择 photo-0.jpg', exact: true }).click();
  await page
    .getByRole('button', { name: '选择 photo-1.jpg', exact: true })
    .click({ modifiers: ['Meta'] });
  await page.getByLabel('搜索照片', { exact: true }).fill('photo-3');
  await expect(page.locator('.hidden-selection')).toHaveText('2 张不在当前结果中');
  await page.getByRole('button', { name: '查看已选', exact: true }).click();
  await expect(page.locator('.photo-card')).toHaveCount(2);
  await expect(page.locator('.photo-card')).toContainText(['photo-0.jpg', 'photo-1.jpg']);
  await page.locator('.selection-bar').getByRole('button', { name: '返回筛选结果' }).click();
  await page.locator('.selection-bar').getByRole('button', { name: '比较', exact: false }).click();
  await expect(page.locator('.photo-pane')).toHaveCount(2);
  await page.keyboard.press('Escape');
  await page.locator('.selection-bar').getByRole('button', { name: '4 星', exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () => (window as any).qa.calls.filter((c: any) => c.cmd === 'set_rating').at(-1)?.args.ids,
      ),
    )
    .toEqual(['photo-0', 'photo-1']);
  await page.getByRole('button', { name: '导出照片副本', exact: true }).click();
  await expect(page.locator('.export-file')).toHaveCount(2);
  await expect(page.getByRole('dialog')).toContainText('含 2 张隐藏选择');
});

test('export offers both modes, retains exact scope, opens the recorded destination and finishes', async ({
  page,
}) => {
  await scene(page);
  await page.getByRole('button', { name: '选择 photo-2.jpg', exact: true }).click();
  await page.getByRole('button', { name: '导出照片副本', exact: true }).click();
  await page.getByRole('tab', { name: '日期城市水印', exact: true }).click();
  await expect(page.getByRole('dialog', { name: '日期城市水印' })).toContainText('1 张');
  await page.getByRole('tab', { name: '原格式副本', exact: true }).click();
  await page.getByRole('button', { name: '选择导出位置', exact: true }).click();
  await expect(page.getByRole('button', { name: '开始导出', exact: true })).toBeEnabled();
  await expect(page.getByRole('dialog')).toContainText('1 张照片 · 2 个媒体文件 · 1 个评分 XMP');
  await expect(page.locator('.export-file')).toContainText('001-photo.mov');
  await expect(page.locator('.directory-path > strong')).toHaveText('Long folder name 中文');
  await expect(page.locator('.directory-path code')).toBeHidden();
  await page.getByRole('button', { name: '开始导出', exact: true }).click();
  await expect(page.getByRole('button', { name: '开始导出', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '打开导出文件夹', exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () => (window as any).qa.calls.find((c: any) => c.cmd === 'open_export_folder')?.args,
      ),
    )
    .toEqual({ folderId: 1, id: 'owned-job' });
  await page.getByRole('button', { name: '完成', exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  expect(
    await page.evaluate(
      () => (window as any).qa.calls.filter((c: any) => c.cmd === 'confirm_export').length,
    ),
  ).toBe(1);
});

test('native menu commands use the same modal guards, search and export scope', async ({
  page,
}) => {
  await scene(page);
  await page.evaluate(() => (window as any).qa.command('search'));
  await expect(page.getByLabel('搜索照片', { exact: true })).toBeFocused();
  await page.getByLabel('搜索照片', { exact: true }).fill('photo-2');
  await page.evaluate(() => (window as any).qa.command('export'));
  await expect(page.getByRole('dialog', { name: '导出照片副本' })).toBeVisible();
  await expect(page.locator('.export-file')).toHaveCount(1);
  const before = await page.evaluate(() => (window as any).qa.calls.length);
  await page.evaluate(() => (window as any).qa.command('open'));
  expect(await page.evaluate(() => (window as any).qa.calls.length)).toBe(before);
});

for (const theme of ['light', 'dark'])
  test(`every tool dialog has a usable exit and bounded layout at 960x640 in ${theme}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 960, height: 640 });
    await scene(page);
    await page.evaluate((t) => (document.documentElement.dataset.theme = t), theme);
    for (const [button, dialog] of [
      ['安全整理', '安全整理'],
      ['评分同步', '评分同步'],
      ['资料库备份', '资料库备份'],
      ['重新关联目录', '重新关联目录'],
      ['日期城市水印', '日期城市水印'],
    ]) {
      await tools(page).click();
      await page.getByRole('button', { name: button, exact: true }).click();
      const panel = page.getByRole('dialog', { name: dialog, exact: true });
      await expect(panel).toBeVisible();
      expect(
        await panel.evaluate((el) => {
          const r = el.getBoundingClientRect();
          return r.top >= 0 && r.bottom <= innerHeight && el.scrollWidth <= el.clientWidth + 1;
        }),
      ).toBe(true);
      const close = panel.locator('button[aria-label^="关闭"]');
      await expect(close).toBeInViewport({ ratio: 1 });
      await page.screenshot({ path: `test-results/experience-${theme}-${button}.png` });
      await page.keyboard.press('Escape');
      await expect(panel).toHaveCount(0);
      await expect(tools(page)).toBeFocused();
    }
  });

test('failed folder selection offers a local retry without exposing diagnostics by default', async ({
  page,
}) => {
  await scene(page);
  await page.evaluate(() => ((window as any).qa.failOpen = true));
  await page.getByRole('button', { name: '打开文件夹', exact: true }).click();
  const error = page.getByRole('alert');
  await expect(error).toContainText('未能完成当前操作');
  await expect(error.locator('.technical-detail')).toBeHidden();
  await error.getByText('详细信息', { exact: true }).click();
  await expect(error).toContainText('EACCES');
  await page.evaluate(() => ((window as any).qa.failOpen = false));
  await error.getByRole('button', { name: '重新选择文件夹', exact: true }).click();
  await expect(error).toHaveCount(0);
});

test('already saved quit is quiet while a slow checkpoint shows progress and remains cancellable', async ({
  page,
}) => {
  await scene(page);
  await expect
    .poll(() =>
      page.evaluate(() =>
        (window as any).qa.calls.some((call: { cmd: string }) => call.cmd === 'save_workspace'),
      ),
    )
    .toBe(true);
  await page.evaluate(() => (window as any).qa.exit(1));
  await expect
    .poll(() =>
      page.evaluate(
        () => (window as any).qa.calls.filter((c: any) => c.cmd === 'resolve_style_exit').length,
      ),
    )
    .toBe(1);
  await expect(page.getByRole('dialog', { name: '保存后退出' })).not.toBeVisible();
  await page.evaluate(() => ((window as any).qa.holdExit = true));
  await page.getByLabel('搜索照片', { exact: true }).fill('photo-1');
  await page.evaluate(() => (window as any).qa.exit(2));
  const exit = page.getByRole('dialog', { name: '保存后退出' });
  await expect(exit).toBeVisible();
  await exit.getByRole('button', { name: '返回应用', exact: true }).click();
  await page.evaluate(() => (window as any).qa.releaseExit());
  expect(
    await page.evaluate(
      () =>
        (window as any).qa.calls.filter(
          (c: any) => c.cmd === 'resolve_style_exit' && c.args.token === 2 && c.args.proceed,
        ).length,
    ),
  ).toBe(0);
});

test('export tabs support arrow navigation and keep focus inside the selected editor', async ({
  page,
}) => {
  await scene(page);
  await page.getByRole('button', { name: '导出照片副本', exact: true }).click();
  await page.getByRole('tab', { name: '原格式副本', exact: true }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('tab', { name: '日期城市水印', exact: true })).toBeFocused();
  await page.keyboard.press('ArrowLeft');
  await expect(page.getByRole('tab', { name: '原格式副本', exact: true })).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(page.getByRole('button', { name: '关闭导出副本', exact: true })).toBeFocused();
});

test('informational text tokens keep normal-text contrast in both themes', async ({ page }) => {
  await scene(page);
  for (const theme of ['light', 'dark']) {
    await page.evaluate((t) => (document.documentElement.dataset.theme = t), theme);
    const ratios = await page.evaluate(() => {
      const css = getComputedStyle(document.documentElement);
      const lum = (hex: string) => {
        const normalized = hex.trim().slice(1);
        const expanded =
          normalized.length === 3
            ? normalized
                .split('')
                .map((c) => c + c)
                .join('')
            : normalized;
        const rgb = expanded
          .match(/../g)!
          .map((c) => parseInt(c, 16) / 255)
          .map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
        return rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
      };
      return ['--muted', '--subtle', '--danger'].flatMap((f) =>
        ['--surface', '--bg', '--surface-soft'].map((b) => {
          const fg = lum(css.getPropertyValue(f)),
            bg = lum(css.getPropertyValue(b));
          return {
            pair: `${f}/${b}`,
            ratio: (Math.max(fg, bg) + 0.05) / (Math.min(fg, bg) + 0.05),
          };
        }),
      );
    });
    for (const pair of ratios)
      expect(pair.ratio, `${theme} ${pair.pair}`).toBeGreaterThanOrEqual(4.5);
  }
});
