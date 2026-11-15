import { test, expect, type Page } from '@playwright/test';

async function scene(page: Page) {
  await page.addInitScript(() => {
    const read = (key: string) => JSON.parse(localStorage.getItem(key) || 'null');
    const controls = {
      requestExit: (_token: number) => {},
      resolutions: [] as { token: number; proceed: boolean }[],
      holdMutation: false,
      releaseMutation: () => {},
      failMutation: false,
      loseMutationReply: false,
      failScan: false,
      cancelScan: false,
      failScanAfterDone: false,
      missingPhoto: false,
      holdWorkspace: false,
      releaseWorkspace: () => {},
      failWorkspace: false,
      workspaceStarted: 0,
      mutationCalls: 0,
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
      rating: read(`rating-${i}`),
      revision: 0,
      format: 'jpg',
    }));
    let before: { id: string; rating: number | null }[] = [];
    Object.assign(window, {
      isTauri: true,
      __reviewExit: controls,
      __TAURI_INTERNALS__: {
        transformCallback: () => 1,
        unregisterCallback: () => {},
        convertFileSrc: () =>
          `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="48" height="32"><path fill="#617b68" d="M0 0h48v32H0z"/></svg>')}`,
        invoke: async (cmd: string, args: any = {}) => {
          if (cmd === 'register_style_exit_guard') {
            controls.requestExit = (token) => args.onRequest.onmessage(token);
            return;
          }
          if (cmd === 'resolve_style_exit') {
            controls.resolutions.push({ token: args.token, proceed: args.proceed });
            return;
          }
          if (cmd === 'load_workspace') return read('workspace');
          if (cmd === 'save_workspace') {
            controls.workspaceStarted++;
            if (controls.holdWorkspace)
              await new Promise<void>((r) => {
                controls.releaseWorkspace = r;
              });
            if (controls.failWorkspace) throw Error('checkpoint denied');
            localStorage.setItem(
              'workspace',
              JSON.stringify({ folderId: 1, name: 'Generated', view: args.view }),
            );
            return;
          }
          if (cmd === 'choose_folder' || cmd === 'resume_workspace')
            return { id: 1, name: 'Generated', demo: false };
          if (cmd === 'cancel_scan') return;
          if (cmd === 'scan_folder') {
            if (controls.failScan) throw Error('scan unavailable');
            // IPC gives the UI values, not references to the backend catalog.
            const scanned = assets
              .filter((a) => !controls.missingPhoto || a.id !== 'photo-0')
              .map((a) => ({ ...a }));
            args.onEvent.onmessage({ kind: 'batch', items: scanned });
            args.onEvent.onmessage(
              controls.cancelScan
                ? { kind: 'cancelled' }
                : { kind: 'done', count: scanned.length, warnings: [] },
            );
            if (controls.failScanAfterDone) throw Error('scan reply lost');
            return;
          }
          if (cmd === 'set_rating' || cmd === 'undo_rating') {
            controls.mutationCalls++;
            if (controls.holdMutation)
              await new Promise<void>((r) => {
                controls.releaseMutation = r;
              });
            if (controls.failMutation) throw Error('rating reply lost');
            const changes = (cmd === 'set_rating' ? args.ids : before.map((x) => x.id)).map(
              (id: string) => {
                const a = assets.find((x) => x.id === id)!;
                const after =
                  cmd === 'set_rating' ? args.rating : before.find((x) => x.id === id)!.rating;
                const change = { id, before: a.rating, after, revision: ++a.revision };
                a.rating = after;
                localStorage.setItem(`rating-${id.split('-')[1]}`, JSON.stringify(after));
                return change;
              },
            );
            if (cmd === 'set_rating')
              before = changes.map((c: any) => ({ id: c.id, rating: c.before }));
            if (controls.loseMutationReply) throw Error('committed reply lost');
            return { token: 'change', changes };
          }
          if (cmd === 'application_info')
            return {
              version: '0.4.0',
              buildId: 'exit-review-test',
              channel: 'local-trial',
              isolated: true,
              dataDirectory: '/generated/runtime',
              executable: '/generated/App',
            };
          throw Error(`Unhandled ${cmd}`);
        },
      },
    });
  });
  await page.goto('/');
  await page
    .locator('.welcome')
    .getByRole('button', { name: '打开照片文件夹', exact: true })
    .click();
  await page.getByRole('button', { name: '查看 photo-0.jpg', exact: true }).dblclick();
  await expect(page.getByText('审片位置已保存', { exact: true })).toBeVisible();
}
const resolutions = (page: Page) => page.evaluate(() => (window as any).__reviewExit.resolutions);

for (const theme of ['light', 'dark']) {
  test(`rating recovery remains accessible in a small ${theme} window`, async ({ page }) => {
    await page.setViewportSize({ width: 960, height: 640 });
    await page.addInitScript((value) => localStorage.setItem('moment-grove-theme', value), theme);
    await scene(page);
    await page.evaluate(() => {
      (window as any).__reviewExit.failMutation = true;
    });
    await page.locator('.pane-name').click();
    await page.keyboard.press('4');
    const alert = page.getByRole('alert');
    await expect(alert).toContainText('重新扫描会回到照片列表并清空本次撤销记录');
    const recover = alert.getByRole('button', { name: '重新扫描核对评分', exact: true });
    await expect(recover).toBeInViewport({ ratio: 1 });
    await expect(recover).toBeEnabled();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
      true,
    );
    await page.screenshot({ path: `.local/rating-recovery-stage/error-${theme}-small.png` });
    await page.evaluate(() => {
      (window as any).__reviewExit.failMutation = false;
    });
    await recover.click();
    await expect(page.getByRole('alert')).toHaveCount(0);
  });
}

test('quit during undo waits and restores the original photo and rating on relaunch', async ({
  page,
}) => {
  await scene(page);
  await page.locator('.pane-name').click();
  await page.keyboard.press('5');
  await expect(page.locator('.photo-pane')).toContainText('photo-1.jpg');
  await page.evaluate(() => {
    (window as any).__reviewExit.holdMutation = true;
  });
  await page.getByRole('button', { name: '撤销评分', exact: true }).click();
  await page.evaluate(() => (window as any).__reviewExit.requestExit(3));
  await expect(page.getByRole('dialog').getByRole('status')).toContainText('评分');
  expect(await resolutions(page)).toEqual([]);
  await page.evaluate(() => (window as any).__reviewExit.releaseMutation());
  await expect.poll(() => resolutions(page)).toEqual([{ token: 3, proceed: true }]);
  await page.reload();
  await expect(page.locator('.photo-pane')).toContainText('photo-0.jpg');
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('rating-0')!))).toBeNull();
});

test('return while a rating is pending cancels exit and late completion cannot quit', async ({
  page,
}) => {
  await scene(page);
  await page.evaluate(() => {
    (window as any).__reviewExit.holdMutation = true;
  });
  await page.locator('.pane-name').click();
  await page.keyboard.press('4');
  await page.evaluate(() => (window as any).__reviewExit.requestExit(4));
  await page.getByRole('dialog').getByRole('button', { name: '返回应用' }).click();
  await page.evaluate(() => (window as any).__reviewExit.releaseMutation());
  await expect(page.getByText('审片位置已保存', { exact: true })).toBeVisible();
  expect(await resolutions(page)).toEqual([{ token: 4, proceed: false }]);
  await page.evaluate(() => (window as any).__reviewExit.requestExit(5));
  await expect
    .poll(() => resolutions(page))
    .toEqual([
      { token: 4, proceed: false },
      { token: 5, proceed: true },
    ]);
});

test('uncertain rating reply blocks exit without automatically replaying the mutation', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 640 });
  await page.addInitScript(() => localStorage.setItem('moment-grove-theme', 'dark'));
  await scene(page);
  await page.evaluate(() => {
    (window as any).__reviewExit.failMutation = true;
  });
  await page.locator('.pane-name').click();
  await page.keyboard.press('4');
  await expect(page.getByRole('alert')).toContainText('rating reply lost');
  await page.evaluate(() => (window as any).__reviewExit.requestExit(6));
  const exit = page.getByRole('dialog');
  await expect(exit.getByRole('alert')).toContainText('评分或撤销结果尚未确认');
  for (const button of await exit.getByRole('button').all()) await expect(button).toBeInViewport();
  await exit.screenshot({ path: '.local/review-exit-stage/rating-error-dark-small.png' });
  await exit.getByRole('button', { name: '重试保存并退出' }).click();
  await expect(exit.getByRole('alert')).toContainText('不会自动重复评分或撤销');
  expect(await resolutions(page)).toEqual([]);
  expect(await page.evaluate(() => (window as any).__reviewExit.mutationCalls)).toBe(1);
  await exit.getByRole('button', { name: '返回应用' }).click();
  await page.evaluate(() => {
    (window as any).__reviewExit.failMutation = false;
  });
  await page.getByRole('button', { name: '重新扫描核对评分', exact: true }).click();
  await expect(page.getByRole('alert')).toHaveCount(0);
  await page.getByRole('button', { name: '查看 photo-0.jpg', exact: true }).dblclick();
  await page.locator('.pane-name').click();
  await page.keyboard.press('5');
  await expect(page.locator('.photo-pane')).toContainText('photo-1.jpg');
  await page.evaluate(() => (window as any).__reviewExit.requestExit(7));
  await expect
    .poll(() => resolutions(page))
    .toEqual([
      { token: 6, proceed: false },
      { token: 7, proceed: true },
    ]);
});

test('a committed rating with a lost reply requires a complete read before writes or quit', async ({
  page,
}) => {
  await scene(page);
  await page.evaluate(() => {
    (window as any).__reviewExit.loseMutationReply = true;
  });
  await page.locator('.pane-name').click();
  await page.keyboard.press('4');
  await expect(page.getByRole('alert')).toContainText('committed reply lost');
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('rating-0')!))).toBe(4);
  await expect(page.getByRole('button', { name: '5 星', exact: true })).toBeDisabled();
  for (const name of ['导出照片副本', '评分同步', '安全整理', '资料库备份', '打开文件夹'])
    await expect(page.getByRole('button', { name, exact: true })).toBeDisabled();
  await page.locator('.pane-name').click();
  await page.keyboard.press('5');
  await page.keyboard.press('Meta+z');
  expect(await page.evaluate(() => (window as any).__reviewExit.mutationCalls)).toBe(1);
  await page.evaluate(() => (window as any).__reviewExit.requestExit(20));
  await expect(page.getByRole('dialog').getByRole('alert')).toContainText('不会自动重复');
  await page.getByRole('dialog').getByRole('button', { name: '返回应用' }).click();

  await page.evaluate(() => {
    (window as any).__reviewExit.failScan = true;
  });
  await page.getByRole('button', { name: '重新扫描核对评分', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('scan unavailable');
  await expect(page.getByRole('button', { name: '评分同步', exact: true })).toBeDisabled();
  await page.evaluate(() => {
    const c = (window as any).__reviewExit;
    c.failScan = false;
    c.failScanAfterDone = true;
  });
  await page.getByRole('button', { name: '重新扫描核对评分', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('scan reply lost');
  await expect(page.getByRole('button', { name: '评分同步', exact: true })).toBeDisabled();
  await page.evaluate(() => {
    const c = (window as any).__reviewExit;
    c.failScanAfterDone = false;
    c.cancelScan = true;
  });
  await page.getByRole('button', { name: '重新扫描核对评分', exact: true }).click();
  await expect(page.locator('.statusbar')).toContainText('扫描已停止');
  await expect(page.getByRole('alert')).toContainText('committed reply lost');
  await page.evaluate(() => {
    const c = (window as any).__reviewExit;
    c.cancelScan = false;
    c.missingPhoto = true;
  });
  await page.getByRole('button', { name: '重新扫描核对评分', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('此次操作的部分照片未能载入');
  await expect(page.getByRole('button', { name: '评分同步', exact: true })).toBeDisabled();
  await page.evaluate(() => {
    (window as any).__reviewExit.missingPhoto = false;
  });
  await page.getByRole('button', { name: '重新扫描核对评分', exact: true }).click();
  await expect(page.getByRole('alert')).toHaveCount(0);
  await expect(
    page
      .locator('.photo-card')
      .filter({ has: page.getByRole('button', { name: '查看 photo-0.jpg', exact: true }) }),
  ).toContainText('4');
  expect(await page.evaluate(() => (window as any).__reviewExit.mutationCalls)).toBe(1);
  await page.evaluate(() => (window as any).__reviewExit.requestExit(21));
  await expect
    .poll(() => resolutions(page))
    .toEqual([
      { token: 20, proceed: false },
      { token: 21, proceed: true },
    ]);
});

test('a committed undo with a lost reply reconciles without repeating undo or keeping old tokens', async ({
  page,
}) => {
  await scene(page);
  await page.locator('.pane-name').click();
  await page.keyboard.press('5');
  await expect(page.locator('.photo-pane')).toContainText('photo-1.jpg');
  await page.evaluate(() => {
    (window as any).__reviewExit.loseMutationReply = true;
  });
  await page.getByRole('button', { name: '撤销评分', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('committed reply lost');
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('rating-0')!))).toBeNull();
  await expect(page.getByRole('button', { name: '撤销评分', exact: true })).toBeDisabled();
  await page.getByRole('button', { name: '重新扫描当前文件夹', exact: true }).click();
  await expect(page.getByRole('alert')).toHaveCount(0);
  await expect(page.getByRole('button', { name: '撤销评分', exact: true })).toBeDisabled();
  expect(await page.evaluate(() => (window as any).__reviewExit.mutationCalls)).toBe(2);
  await page.evaluate(() => {
    (window as any).__reviewExit.loseMutationReply = false;
  });
  await page.getByRole('button', { name: '查看 photo-0.jpg', exact: true }).dblclick();
  await expect(page.getByRole('button', { name: '5 星', exact: true })).not.toHaveClass(/rated/);
  await page.locator('.pane-name').click();
  await page.keyboard.press('2');
  await expect(page.locator('.photo-pane')).toContainText('photo-1.jpg');
  expect(await page.evaluate(() => (window as any).__reviewExit.mutationCalls)).toBe(3);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('rating-0')!))).toBe(2);
});

test('a failed rating still drains an in-flight checkpoint before offering an exit choice', async ({
  page,
}) => {
  await scene(page);
  await page.evaluate(() => {
    const c = (window as any).__reviewExit;
    c.failMutation = true;
    c.holdWorkspace = true;
  });
  await page.locator('.pane-name').click();
  await page.keyboard.press('4');
  await expect(page.getByRole('alert')).toContainText('rating reply lost');
  await page.evaluate(() => (window as any).__reviewExit.requestExit(9));
  const exit = page.getByRole('dialog');
  await expect(exit.getByRole('status')).toContainText('正在保存');
  await expect(exit.getByRole('button', { name: '放弃未保存的修改并退出' })).toHaveCount(0);
  expect(await resolutions(page)).toEqual([]);
  await page.evaluate(() => {
    const c = (window as any).__reviewExit;
    c.holdWorkspace = false;
    c.releaseWorkspace();
  });
  await expect(exit.getByRole('alert')).toContainText('评分或撤销结果尚未确认');
  expect(await page.evaluate(() => (window as any).__reviewExit.mutationCalls)).toBe(1);
});

test('exit drains rapid search, sorting, density and theme edits to the final checkpoint', async ({
  page,
}) => {
  await scene(page);
  await page.getByRole('button', { name: '返回照片', exact: true }).click();
  await page.evaluate(() => {
    (window as any).__reviewExit.holdWorkspace = true;
  });
  await page.getByRole('textbox', { name: '搜索照片' }).fill('photo-');
  await page.getByRole('combobox', { name: '排序方式' }).selectOption('date');
  await page.getByRole('slider', { name: '缩略图大小' }).fill('340');
  await page.getByRole('button', { name: '切换暗色主题' }).click();
  await page.evaluate(() => (window as any).__reviewExit.requestExit(8));
  await expect(page.getByRole('dialog')).toBeVisible();
  expect(await resolutions(page)).toEqual([]);
  await page.evaluate(() => {
    const c = (window as any).__reviewExit;
    c.holdWorkspace = false;
    c.releaseWorkspace();
  });
  await expect.poll(() => resolutions(page)).toEqual([{ token: 8, proceed: true }]);
  await page.reload();
  await expect(page.getByRole('textbox', { name: '搜索照片' })).toHaveValue('photo-');
  await expect(page.getByRole('combobox', { name: '排序方式' })).toHaveValue('date');
  await expect(page.getByRole('slider', { name: '缩略图大小' })).toHaveValue('340');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
});

test('quit waits for pending rating and its auto-advanced checkpoint before acknowledging', async ({
  page,
}) => {
  await scene(page);
  await page.evaluate(() => {
    (window as any).__reviewExit.holdMutation = true;
    (window as any).__reviewExit.holdWorkspace = true;
  });
  await page.locator('.pane-name').click();
  await page.keyboard.press('4');
  await page.evaluate(() => (window as any).__reviewExit.requestExit(1));
  await expect(page.getByRole('dialog')).toBeVisible();
  expect(await resolutions(page)).toEqual([]);
  await page.evaluate(() => {
    const c = (window as any).__reviewExit;
    c.releaseMutation();
  });
  await expect(page.locator('.photo-pane')).toContainText('photo-1.jpg');
  expect(await resolutions(page)).toEqual([]);
  await page.evaluate(() => {
    const c = (window as any).__reviewExit;
    c.holdWorkspace = false;
    c.releaseWorkspace();
  });
  await expect.poll(() => resolutions(page)).toEqual([{ token: 1, proceed: true }]);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('workspace')!))).toMatchObject({
    view: { active: 'photo-1', mode: 'review' },
  });
  await page.reload();
  await expect(page.locator('.photo-pane')).toContainText('photo-1.jpg');
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('rating-0')!))).toBe(4);
});

test('workspace failure prevents quit and retry saves the retained checkpoint', async ({
  page,
}) => {
  await scene(page);
  await page.evaluate(() => {
    (window as any).__reviewExit.failWorkspace = true;
  });
  await page.getByRole('button', { name: '下一张', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('审片位置');
  await page.evaluate(() => (window as any).__reviewExit.requestExit(2));
  const exit = page.getByRole('dialog');
  await expect(exit.getByRole('alert')).toContainText('审片位置');
  expect(await resolutions(page)).toEqual([]);
  await page.evaluate(() => {
    (window as any).__reviewExit.failWorkspace = false;
  });
  await exit.getByRole('button', { name: '重试保存并退出' }).click();
  await expect.poll(() => resolutions(page)).toEqual([{ token: 2, proceed: true }]);
  await page.reload();
  await expect(page.locator('.photo-pane')).toContainText('photo-1.jpg');
});
