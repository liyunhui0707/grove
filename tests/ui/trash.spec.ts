import { test, expect, type Page } from '@playwright/test';
async function scene(page: Page) {
  await page.route('http://grove-qa.invalid/**', (r) =>
    r.fulfill({
      contentType: 'image/svg+xml',
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><path fill="#637b68" d="M0 0h40v30H0z"/></svg>',
    }),
  );
  await page.addInitScript(() => {
    const callbacks = new Map<number, Function>();
    let count = 0;
    const assets = Array.from({ length: 4 }, (_, n) => ({
      id: `photo-${n}`,
      folderId: 1,
      name: `generated-${n}.jpg`,
      relative: `generated-${n}.jpg`,
      width: 40,
      height: 30,
      bytes: 100,
      modified: 0,
      captured: null,
      rating: n < 2 ? 0 : n === 2 ? 4 : null,
      revision: 1,
      format: 'jpg',
    }));
    const qa = {
      calls: [] as { cmd: string; args: any }[],
      hold: false,
      release: () => {},
      fail: false,
    };
    let report: any = null;
    let history: any[] = [];
    Object.assign(window, {
      isTauri: true,
      qa,
      __TAURI_INTERNALS__: {
        transformCallback: (fn: Function) => {
          callbacks.set(++count, fn);
          return count;
        },
        unregisterCallback: (id: number) => callbacks.delete(id),
        convertFileSrc: (id: string) => `http://grove-qa.invalid/${id}`,
        invoke: async (cmd: string, args: any = {}) => {
          qa.calls.push({ cmd, args });
          if (cmd === 'plugin:event|listen') return 1;
          if (
            [
              'plugin:event|unlisten',
              'register_style_exit_guard',
              'record_startup_stage',
              'cancel_scan',
              'save_workspace',
              'cancel_trash',
            ].includes(cmd)
          )
            return;
          if (['load_workspace', 'load_watermark_style'].includes(cmd)) return null;
          if (cmd === 'choose_folder') return { id: 1, name: 'Generated library', demo: false };
          if (cmd === 'scan_folder') {
            args.onEvent.onmessage({ kind: 'batch', items: assets.filter((a: any) => !a.trashed) });
            args.onEvent.onmessage({
              kind: 'done',
              count: assets.filter((a: any) => !a.trashed).length,
              warnings: [],
            });
            return;
          }
          if (cmd === 'trash_history') return history;
          if (cmd === 'prepare_trash') {
            if (qa.fail) throw Error('XMP 已变化，请重新扫描');
            report = {
              id: 'owned-generated-job',
              created: 1,
              sourceFolder: 1,
              source: '/generated/very-long-source/中文/source',
              status: 'ready',
              completed: 0,
              message: null,
              items: args.targets.map((t: any) => ({
                name: assets.find((a) => a.id === t.id)!.name,
                status: 'pending',
                resources: [
                  {
                    name: assets.find((a) => a.id === t.id)!.name,
                    status: 'pending',
                    destination: null,
                  },
                  { name: 'generated.xmp', status: 'pending', destination: null },
                ],
              })),
            };
            return { token: 'single-use-confirmation', report };
          }
          if (cmd === 'confirm_trash') {
            if (qa.hold) await new Promise<void>((resolve) => (qa.release = resolve));
            report = {
              ...report,
              status: 'completed',
              completed: report.items.length,
              items: report.items.map((a: any) => ({
                ...a,
                status: 'trashed',
                resources: a.resources.map((f: any) => ({
                  ...f,
                  status: 'trashed',
                  destination: `/generated/.Trash/${f.name}`,
                })),
              })),
            };
            for (const asset of assets)
              if (report.items.some((r: any) => r.name === asset.name))
                Object.assign(asset, { trashed: true });
            history = [report];
            args.onEvent.onmessage(report);
            return report;
          }
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
test('reject view requires explicit selection; cancel never moves; confirm refreshes counts and retains filter', async ({
  page,
}) => {
  await scene(page);
  await page.getByRole('button', { name: '淘汰候选', exact: true }).click();
  await expect(page.locator('.photo-card')).toHaveCount(2);
  const trigger = page.getByRole('button', { name: '移到废纸篓', exact: true });
  await expect(trigger).toBeDisabled();
  await page.getByRole('button', { name: '全选当前结果', exact: true }).click();
  await page.getByRole('button', { name: '移到废纸篓 · 已选 2 张', exact: true }).click();
  let dialog = page.getByRole('dialog', { name: '移到废纸篓', exact: true });
  await expect(
    dialog.getByRole('button', { name: '确认移到废纸篓 · 2 张', exact: true }),
  ).toBeEnabled();
  await expect(dialog.locator('.trash-files > li')).toHaveCount(2);
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  expect(
    await page.evaluate(
      () => (window as any).qa.calls.filter((c: any) => c.cmd === 'confirm_trash').length,
    ),
  ).toBe(0);
  await page.getByRole('button', { name: '移到废纸篓 · 已选 2 张', exact: true }).click();
  dialog = page.getByRole('dialog', { name: '移到废纸篓', exact: true });
  await dialog.getByRole('button', { name: '确认移到废纸篓 · 2 张', exact: true }).click();
  await expect(dialog.getByRole('status')).toContainText('2 / 2');
  await dialog.getByRole('button', { name: '关闭废纸篓操作' }).click();
  await expect(page.locator('.photo-card')).toHaveCount(0);
  await expect(page.locator('.nav-item.active')).toHaveText('淘汰候选');
  await page.getByRole('button', { name: '操作记录', exact: true }).click();
  await page.getByText('最近操作记录（1）').click();
  await expect(
    page.getByRole('dialog').getByText('已移到废纸篓', { exact: false }).first(),
  ).toBeVisible();
});
test('hidden selections and nonzero ratings cannot be included by changing filter', async ({
  page,
}) => {
  await scene(page);
  await page.getByRole('button', { name: '选择 generated-2.jpg', exact: true }).click();
  await page.getByRole('button', { name: '淘汰候选', exact: true }).click();
  await expect(
    page.getByRole('button', { name: '移到废纸篓 · 已选 1 张', exact: true }),
  ).toBeDisabled();
  await page.getByRole('button', { name: '全选当前结果', exact: true }).click();
  await page.getByRole('button', { name: '移到废纸篓 · 已选 2 张', exact: true }).click();
  await expect(
    page.getByRole('button', { name: '确认移到废纸篓 · 2 张', exact: true }),
  ).toBeEnabled();
  expect(
    await page.evaluate(() =>
      (window as any).qa.calls
        .find((c: any) => c.cmd === 'prepare_trash')
        .args.targets.map((t: any) => t.id),
    ),
  ).toEqual(['photo-0', 'photo-1']);
});
test('failed preparation is explicit and never enables destructive action', async ({ page }) => {
  await scene(page);
  await page.evaluate(() => ((window as any).qa.fail = true));
  await page.getByRole('button', { name: '淘汰候选', exact: true }).click();
  await page.getByRole('button', { name: '全选当前结果', exact: true }).click();
  await page.getByRole('button', { name: '移到废纸篓 · 已选 2 张', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('废纸篓操作未完成');
  await expect(
    page.getByRole('button', { name: '确认移到废纸篓 · 2 张', exact: true }),
  ).toBeDisabled();
});
test('dialog remains usable in a small dark window and traps focus; processing blocks repeated confirmation', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 700 });
  await scene(page);
  await page.getByRole('button', { name: '淘汰候选', exact: true }).click();
  await page.getByRole('button', { name: '全选当前结果', exact: true }).click();
  await page.getByRole('button', { name: '移到废纸篓 · 已选 2 张', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '移到废纸篓', exact: true });
  const confirm = dialog.getByRole('button', { name: '确认移到废纸篓 · 2 张', exact: true });
  await expect(confirm).toBeEnabled();
  await page.evaluate(() => {
    document.documentElement.dataset.theme = 'dark';
    (window as any).qa.hold = true;
  });
  const box = await dialog.boundingBox();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.y).toBeGreaterThanOrEqual(0);
  expect(box!.y + box!.height).toBeLessThanOrEqual(700);
  await confirm.focus();
  await page.keyboard.press('Tab');
  await expect(dialog.getByRole('button', { name: '关闭废纸篓操作' })).toBeFocused();
  await confirm.click();
  await expect(dialog.getByRole('button', { name: '关闭废纸篓操作' })).toBeDisabled();
  await expect(confirm).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(dialog).toBeVisible();
  await page.evaluate(() => (window as any).qa.release());
  await expect(dialog.getByRole('status')).toContainText('2 / 2');
  expect(
    await page.evaluate(
      () => (window as any).qa.calls.filter((c: any) => c.cmd === 'confirm_trash').length,
    ),
  ).toBe(1);
});
