import { test, expect, type Page } from '@playwright/test';

test('unreadable folder offers rescan and recovers without choosing another folder', async ({
  page,
}) => {
  await installMoveMock(page);
  await page.addInitScript(() => {
    const internal = (
      window as unknown as {
        __TAURI_INTERNALS__: {
          invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
        };
      }
    ).__TAURI_INTERNALS__;
    const invoke = internal.invoke;
    let scans = 0;
    internal.invoke = async (cmd, args) => {
      if (cmd === 'scan_folder' && ++scans === 1)
        throw Error('文件夹暂时离线，请重新扫描或选择文件夹');
      return invoke(cmd, args);
    };
  });
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  await expect(page.getByRole('heading', { name: '扫描未完成', exact: true })).toBeVisible();
  await expect(page.getByRole('alert')).toContainText('文件夹暂时离线');
  await page.screenshot({ path: 'test-results/scan-unavailable-light.png' });
  await page.getByRole('button', { name: '重新扫描当前文件夹', exact: true }).click();
  await expect(page.locator('.photo-card')).toHaveCount(2);
  await expect(page.getByRole('alert')).toHaveCount(0);
  const calls = await page.evaluate(
    () => (window as unknown as { __moveCalls: { cmd: string }[] }).__moveCalls,
  );
  expect(calls.filter((c) => c.cmd === 'create_demo')).toHaveLength(1);
});

test('stopped scan labels partial results and can restart', async ({ page }) => {
  await installMoveMock(page);
  await page.addInitScript(() => {
    const internal = (
      window as unknown as {
        __TAURI_INTERNALS__: {
          invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
        };
      }
    ).__TAURI_INTERNALS__;
    const invoke = internal.invoke;
    let scans = 0;
    internal.invoke = async (cmd, args) => {
      if (cmd === 'scan_folder' && ++scans === 1) {
        const channel = args!.onEvent as { onmessage: (event: unknown) => void };
        return invoke(cmd, {
          ...args,
          onEvent: {
            onmessage: (event: { kind: string; items: unknown[] }) => {
              channel.onmessage(
                event.kind === 'batch'
                  ? { ...event, items: event.items.slice(0, 1) }
                  : { kind: 'cancelled' },
              );
            },
          },
        });
      }
      return invoke(cmd, args);
    };
  });
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  await expect(page.locator('.photo-card')).toHaveCount(1);
  await expect(page.locator('.statusbar')).toContainText('扫描已停止');
  await page.getByRole('button', { name: '重新扫描当前文件夹', exact: true }).click();
  await expect(page.locator('.photo-card')).toHaveCount(2);
  await expect(page.locator('.statusbar')).not.toContainText('扫描已停止');
});

test('readable empty folder explains supported formats instead of search filters', async ({
  page,
}) => {
  await installMoveMock(page);
  await page.addInitScript(() => {
    const internal = (
      window as unknown as {
        __TAURI_INTERNALS__: {
          invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
        };
      }
    ).__TAURI_INTERNALS__;
    const invoke = internal.invoke;
    internal.invoke = async (cmd, args) => {
      if (cmd === 'scan_folder') {
        (args!.onEvent as { onmessage: (event: unknown) => void }).onmessage({
          kind: 'done',
          count: 0,
          warnings: [],
        });
        return;
      }
      return invoke(cmd, args);
    };
  });
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  await expect(page.getByRole('heading', { name: '未找到支持的照片', exact: true })).toBeVisible();
  await expect(page.locator('.empty-results')).toContainText('JPEG、PNG、WebP、HEIC');
  await page.getByLabel('切换暗色主题').click();
  await page.setViewportSize({ width: 960, height: 640 });
  await page.screenshot({ path: 'test-results/scan-empty-dark-small.png' });
});

test('failed native preview can retry without changing selection or rating', async ({ page }) => {
  await installMoveMock(page);
  await page.addInitScript(() => {
    const internals = (
      window as unknown as { __TAURI_INTERNALS__: { convertFileSrc: (id: string) => string } }
    ).__TAURI_INTERNALS__;
    internals.convertFileSrc = (id) => `http://grove.localhost/generated-preview/${id}`;
  });
  let failed = false;
  await page.route('http://grove.localhost/generated-preview/**', async (route) => {
    if (
      !route.request().url().includes('size=480') &&
      !route.request().url().includes('refresh=1')
    ) {
      failed = true;
      await route.fulfill({ status: 422, contentType: 'text/plain', body: '照片暂时离线，可重试' });
    } else {
      await route.fulfill({
        contentType: 'image/svg+xml',
        body: '<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40"><path fill="#617b68" d="M0 0h80v40H0z"/></svg>',
      });
    }
  });
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  await page.getByRole('button', { name: '查看 generated-0.jpg', exact: true }).dblclick();
  const pane = page.getByRole('region', { name: '比较照片 generated-0.jpg' });
  await expect(pane).toContainText('照片暂时离线，可重试');
  await page.screenshot({ path: 'test-results/preview-retry-light.png' });
  await pane.getByRole('button', { name: '重试预览', exact: true }).click();
  await expect(pane.locator('img')).toBeVisible();
  await expect(pane.locator('img')).toHaveJSProperty('naturalWidth', 80);
  expect(failed).toBe(true);
  await expect(pane.getByRole('button', { name: '重试预览' })).toHaveCount(0);
  const changes = await page.evaluate(() =>
    (window as unknown as { __moveCalls: { cmd: string }[] }).__moveCalls.filter((c) =>
      ['rate', 'set_ratings', 'apply_rating'].includes(c.cmd),
    ),
  );
  expect(changes).toEqual([]);
});

test('safe organization isolates only zero and one and protects shortcuts in both themes', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 640 });
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  for (const [name, rating] of [
    ['01', '0 分：淘汰候选'],
    ['02', '1 星'],
  ] as const) {
    await page.getByRole('button', { name: `选择 山海_${name}.jpg`, exact: true }).click();
    await page.locator('.selection-bar').getByLabel(rating, { exact: true }).click();
    await page.getByLabel('取消选择').click();
  }
  const trigger = page.getByRole('button', { name: '安全整理', exact: true });
  await trigger.click();
  const dialog = page.getByRole('dialog', { name: '安全整理' });
  await expect(dialog).toContainText('本次范围 24 张，仅纳入 2 张');
  await expect(dialog.locator('.export-file')).toHaveCount(2);
  await expect(dialog).toContainText('浏览器仅演示清单');
  await expect(dialog.getByRole('button', { name: '选择整理位置' })).toBeDisabled();
  await expect(dialog.getByRole('button', { name: '确认移动并核验' })).toBeInViewport({ ratio: 1 });
  await page.keyboard.press('5');
  await page.keyboard.press('ArrowRight');
  await expect(dialog.locator('.export-file')).toHaveCount(2);
  await dialog.getByRole('button', { name: '移动当前范围', exact: true }).click();
  await expect(dialog.locator('.export-file')).toHaveCount(24);
  await page.keyboard.press('Shift+Tab');
  await expect(dialog.getByRole('button', { name: '低分隔离 · 0–1', exact: true })).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(dialog.getByLabel('关闭安全整理')).toBeFocused();
  await dialog.getByRole('button', { name: '低分隔离 · 0–1', exact: true }).click();
  await page.screenshot({ path: 'test-results/safe-moves-light.png' });
  await page.keyboard.press('Escape');
  await expect(trigger).toBeFocused();
  await page.getByLabel('评分筛选').selectOption('5');
  await expect(page.locator('.photo-card')).toHaveCount(0);
  await page.getByLabel('评分筛选').selectOption('all');
  await page.getByLabel('切换暗色主题').click();
  await trigger.click();
  await expect(dialog.locator('.export-file')).toHaveCount(2);
  await page.screenshot({ path: 'test-results/safe-moves-dark.png' });
});

test('unreviewed selected photos never enter quarantine and recovery opens without a folder', async ({
  page,
}) => {
  await page.goto('/');
  const trigger = page.getByRole('button', { name: '安全整理', exact: true });
  await trigger.click();
  await expect(page.getByRole('dialog', { name: '安全整理' })).toContainText('操作历史与恢复');
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  for (const i of [1, 2])
    await page.getByRole('button', { name: `选择 山海_0${i}.jpg`, exact: true }).click();
  await trigger.click();
  const dialog = page.getByRole('dialog', { name: '安全整理' });
  await expect(dialog).toContainText('本次范围 2 张，仅纳入 0 张');
  await expect(dialog.locator('.export-file')).toHaveCount(0);
  await dialog.getByRole('button', { name: '移动所选照片', exact: true }).click();
  await expect(dialog.locator('.export-file')).toHaveCount(2);
  await page.keyboard.press('Escape');
  await expect(trigger).toBeFocused();
});

async function installMoveMock(page: Page) {
  await page.addInitScript(() => {
    const assets = [0, 1].map((i) => ({
      id: `generated-${i}`,
      folderId: 1,
      name: `generated-${i}.jpg`,
      relative: `generated-${i}.jpg`,
      width: 48,
      height: 32,
      bytes: 100,
      modified: 1,
      captured: null,
      rating: i,
      revision: 1,
      format: 'jpg',
    }));
    const parents = Array.from(
      { length: 7 },
      (_, i) => `相册-${i}-generated-directory-with-long-paths`,
    ).join('/');
    let finish: (() => void) | null = null;
    let recent: unknown[] = [];
    let prepared: Record<string, unknown> | null = null;
    const calls: { cmd: string; args: unknown }[] = [];
    Object.assign(window, {
      isTauri: true,
      __moveCalls: calls,
      __holdMove: false,
      __pauseMove: false,
      __rejectReview: false,
      __cancelNextReview: false,
      __releaseMove: () => finish?.(),
      __TAURI_INTERNALS__: {
        transformCallback: () => 1,
        unregisterCallback: () => {},
        convertFileSrc: () =>
          `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="48" height="32"><path fill="#617b68" d="M0 0h48v32H0z"/></svg>')}`,
        invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
          if (cmd !== 'scan_folder') calls.push({ cmd, args: JSON.parse(JSON.stringify(args)) });
          if (cmd === 'load_workspace') return null;
          if (cmd === 'save_workspace') return;
          if (cmd === 'create_demo') return { id: 1, name: 'Generated test folder', demo: true };
          if (cmd === 'cancel_scan') return;
          if (cmd === 'scan_folder') {
            const channel = args.onEvent as { onmessage: (event: unknown) => void };
            channel.onmessage({ kind: 'batch', items: assets });
            channel.onmessage({ kind: 'done', count: 2, warnings: [] });
            return;
          }
          if (cmd === 'move_history') return recent;
          if (cmd === 'cancel_move') {
            Object.assign(window, { __pauseMove: true });
            return;
          }
          if (cmd === 'choose_move_folder') {
            if ((window as unknown as { __rejectReview: boolean }).__rejectReview)
              throw Error('generated-0.jpg：文件身份已变化，未覆盖或继续移动');
            const controls = window as unknown as { __cancelNextReview: boolean };
            if (controls.__cancelNextReview) {
              controls.__cancelNextReview = false;
              return null;
            }
            prepared = {
              id: 'generated-job',
              reviewToken: crypto.randomUUID(),
              source: `/generated/${parents}/sources`,
              sourceFolder: 1,
              destination: `/generated/${parents}/exports-parent/isolated`,
              kind: args.kind,
              direction: args.restore ? 'restore' : 'forward',
              status: 'ready',
              verified: args.restore ? assets.length : 0,
              items: assets.map((a) => ({
                name: `${parents}/${a.name}`,
                output: a.name,
                rating: a.rating,
                sidecar: false,
                status: args.restore ? 'verified' : 'pending',
                error: null,
              })),
              safetyBackup: args.restore ? 'previous-move-snapshot' : null,
              message: null,
            };
            return prepared;
          }
          if (cmd === 'confirm_move') {
            if (!prepared || args.token !== prepared.reviewToken) throw Error('Stale confirmation');
            const checked = prepared;
            (args.onEvent as { onmessage: (event: unknown) => void }).onmessage({
              ...checked,
              reviewToken: null,
              status: 'running',
              verified: 0,
              items: assets.map((a) => ({
                name: `${parents}/${a.name}`,
                output: a.name,
                rating: a.rating,
                sidecar: false,
                status: 'pending',
                error: null,
              })),
            });
            if ((window as unknown as { __holdMove: boolean }).__holdMove)
              await new Promise<void>((resolve) => {
                finish = resolve;
              });
            const paused = (window as unknown as { __pauseMove: boolean }).__pauseMove;
            const result = {
              ...checked,
              reviewToken: null,
              status: paused
                ? 'paused'
                : checked.direction === 'restore'
                  ? 'restored'
                  : 'completed',
              safetyBackup: 'current-operation-snapshot',
              verified: paused ? 1 : 2,
              items: assets.map((a, i) => ({
                name: `${parents}/${a.name}`,
                output: a.name,
                rating: a.rating,
                sidecar: false,
                status: paused && i > 0 ? 'pending' : 'verified',
                error: null,
              })),
            };
            recent = [result];
            prepared = null;
            return result;
          }
          throw Error('Unexpected mocked command ' + cmd);
        },
      },
    });
  });
}

test('mock IPC requires renewed review and explicit confirmation for move and withdrawal', async ({
  page,
}) => {
  await installMoveMock(page);
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  await page.getByRole('button', { name: '安全整理', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '安全整理' });
  await dialog.getByRole('button', { name: '选择整理位置' }).click();
  await expect(dialog.getByRole('button', { name: '确认移动并核验' })).toBeDisabled();
  await dialog.getByRole('checkbox').check();
  await expect(dialog.getByRole('button', { name: '确认移动并核验' })).toBeEnabled();
  await page.evaluate(() => Object.assign(window, { __cancelNextReview: true }));
  await dialog.getByRole('button', { name: '选择整理位置' }).click();
  await expect(dialog).toContainText('请重新核对清单');
  await expect(dialog.getByRole('progressbar')).toHaveCount(0);
  await expect(dialog.getByRole('checkbox')).toHaveCount(0);
  await expect(dialog.getByRole('button', { name: '确认移动并核验' })).toBeDisabled();
  await dialog.getByRole('button', { name: '移动当前范围', exact: true }).click();
  await expect(dialog.getByRole('checkbox')).toHaveCount(0);
  await expect(dialog.getByRole('button', { name: '确认移动并核验' })).toBeDisabled();
  await dialog.getByRole('button', { name: '选择整理位置' }).click();
  await dialog.getByRole('checkbox').check();
  await dialog.getByRole('button', { name: '确认移动并核验' }).click();
  await expect(dialog.locator('.export-summary')).toContainText('移动已核验');
  await dialog.getByRole('button', { name: '核对撤回' }).click();
  // The backend deliberately retains the previous move's 2 verified rows during review.
  // Reviewing a withdrawal must never look like the withdrawal already happened.
  await expect(dialog.getByRole('progressbar')).toHaveCount(0);
  await expect(dialog.getByText('尚未执行本次撤回。', { exact: false })).toBeInViewport({
    ratio: 1,
  });
  await expect(dialog.getByText('已创建资料库安全快照。', { exact: false })).toHaveCount(0);
  await expect(dialog.locator('.export-file-index svg')).toHaveCount(0);
  await expect(dialog.getByRole('checkbox')).toBeInViewport({ ratio: 1 });
  await expect(dialog.getByRole('button', { name: '确认撤回并核验' })).toBeDisabled();
  await page.evaluate(() => Object.assign(window, { __holdMove: true }));
  await dialog.getByRole('checkbox').check();
  await dialog.getByRole('button', { name: '确认撤回并核验' }).click();
  await expect(dialog.locator('.export-summary')).toContainText('正在撤回与核验');
  await expect(dialog.getByRole('region', { name: '本次整理范围' }).getByRole('status')).toHaveText(
    '正在撤回与核验',
  );
  await expect(dialog.getByRole('progressbar')).toHaveAttribute('value', '0');
  await expect(dialog.getByRole('checkbox')).toHaveCount(0);
  await page.evaluate(() => (window as unknown as { __releaseMove: () => void }).__releaseMove());
  await expect(dialog.locator('.export-summary')).toContainText('已撤回原位置');
  await expect(dialog.getByRole('progressbar')).toHaveAttribute('value', '2');
  const calls = await page.evaluate(
    () =>
      (
        window as unknown as {
          __moveCalls: { cmd: string; args: { targets?: unknown[]; restore?: boolean } }[];
        }
      ).__moveCalls,
  );
  expect(calls.filter((c) => c.cmd === 'confirm_move')).toHaveLength(2);
  expect(calls.filter((c) => c.cmd === 'choose_move_folder').at(-1)?.args.restore).toBe(true);
  expect(calls.filter((c) => c.cmd === 'choose_move_folder')[0].args.targets).toEqual([
    { id: 'generated-0', revision: 1 },
    { id: 'generated-1', revision: 1 },
  ]);
  expect(calls.some((c) => c.cmd === 'set_rating')).toBe(false);
});

test('mock IPC stops once, reopens paused history, and requires a new review after a conflict', async ({
  page,
}) => {
  await installMoveMock(page);
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  await page.getByRole('button', { name: '安全整理', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '安全整理' });
  await dialog.getByRole('button', { name: '选择整理位置' }).click();
  await dialog.getByRole('checkbox').check();
  await page.evaluate(() => Object.assign(window, { __holdMove: true }));
  await dialog.getByRole('button', { name: '确认移动并核验' }).click();
  await expect(dialog.getByRole('button', { name: '关闭安全整理' })).toBeDisabled();
  await dialog.getByRole('button', { name: '停止并保留现场' }).click();
  await expect(dialog.getByRole('button', { name: '完成当前照片后停止…' })).toBeDisabled();
  await page.keyboard.press('Escape');
  await expect(dialog).toBeVisible();
  await page.evaluate(() => (window as unknown as { __releaseMove: () => void }).__releaseMove());
  await expect(dialog.locator('.export-summary')).toContainText('已停止，等待继续或撤回');
  await expect(dialog.getByRole('progressbar')).toHaveAttribute('value', '1');
  await expect(dialog.getByRole('button', { name: '选择整理位置' })).toBeDisabled();
  await page.screenshot({ path: 'test-results/move-paused-light.png' });
  await dialog.getByRole('button', { name: '关闭安全整理' }).click();
  await page.getByRole('button', { name: '安全整理', exact: true }).click();
  await expect(dialog).toContainText('有未完成的整理');
  await dialog.getByRole('button', { name: '继续', exact: true }).click();
  await expect(dialog.getByRole('button', { name: '确认移动并核验' })).toBeDisabled();
  await expect(dialog.getByRole('progressbar')).toHaveCount(0);
  await page.evaluate(() => Object.assign(window, { __holdMove: false, __pauseMove: false }));
  await dialog.getByRole('checkbox').check();
  await dialog.getByRole('button', { name: '确认移动并核验' }).click();
  await expect(dialog.locator('.export-summary')).toContainText('移动已核验');
  await page.evaluate(() => Object.assign(window, { __rejectReview: true }));
  await dialog.getByRole('button', { name: '核对撤回' }).click();
  await expect(dialog.getByRole('alert')).toContainText('generated-0.jpg');
  await expect(dialog.getByRole('alert')).toBeInViewport({ ratio: 1 });
  await expect(dialog.getByRole('region', { name: '本次整理范围' }).getByRole('status')).toHaveText(
    '请查看提示，重新核对',
  );
  await expect(dialog.getByRole('checkbox')).toHaveCount(0);
  await expect(dialog.getByRole('button', { name: '确认移动并核验' })).toBeDisabled();
  await page.screenshot({ path: 'test-results/move-conflict-light.png' });
  await page.evaluate(() => Object.assign(window, { __rejectReview: false }));
  await dialog.getByRole('button', { name: '核对撤回' }).click();
  await expect(dialog.getByRole('alert')).toHaveCount(0);
  await expect(dialog.getByRole('button', { name: '确认撤回并核验' })).toBeDisabled();
  await dialog.getByRole('checkbox').check();
  await dialog.getByRole('button', { name: '确认撤回并核验' }).click();
  await expect(dialog.locator('.export-summary')).toContainText('已撤回原位置');
  const calls = await page.evaluate(
    () => (window as unknown as { __moveCalls: { cmd: string }[] }).__moveCalls,
  );
  expect(calls.filter((c) => c.cmd === 'cancel_move')).toHaveLength(1);
  expect(calls.filter((c) => c.cmd === 'confirm_move')).toHaveLength(3);
});

test('safe organization keeps scope, confirmation and long paths usable in both themes', async ({
  page,
}) => {
  await installMoveMock(page);
  for (const viewport of [
    { width: 1440, height: 960 },
    { width: 960, height: 640 },
  ]) {
    for (const theme of ['light', 'dark']) {
      await page.setViewportSize(viewport);
      await page.goto('/');
      await page.getByRole('button', { name: '体验示例图库' }).click();
      const switchTheme = page.getByLabel(theme === 'dark' ? '切换暗色主题' : '切换浅色主题');
      if (await switchTheme.isVisible()) await switchTheme.click();
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
      await page.getByRole('button', { name: '安全整理', exact: true }).click();
      const dialog = page.getByRole('dialog', { name: '安全整理' });
      const scope = dialog.getByRole('region', { name: '本次整理范围' });
      await expect(scope).toContainText('本次隔离 2 张');
      await expect(scope).toContainText('当前筛选结果');
      await dialog.getByRole('button', { name: '移动当前范围', exact: true }).click();
      await expect(scope).toContainText('本次移动 2 张');
      await dialog.getByRole('button', { name: '选择整理位置' }).click();
      await expect(dialog.getByRole('checkbox')).toBeInViewport({ ratio: 1 });
      await expect(scope).toBeInViewport({ ratio: 1 });
      await expect(dialog.getByRole('progressbar')).toHaveCount(0);
      await expect(dialog.getByRole('button', { name: '确认移动并核验' })).toBeInViewport({
        ratio: 1,
      });
      await dialog.locator('.move-route').scrollIntoViewIfNeeded();
      const overflow = await dialog.evaluate((el) => {
        const box = el.getBoundingClientRect();
        return [
          el,
          ...el.querySelectorAll('.move-body, .move-route, .export-file-list, .move-confirm'),
        ].map((node) => {
          const r = node.getBoundingClientRect();
          return {
            horizontal: node.scrollWidth > node.clientWidth + 1,
            escapes: r.left < box.left - 1 || r.right > box.right + 1,
          };
        });
      });
      expect(overflow.every((r) => !r.horizontal && !r.escapes)).toBe(true);
      await page.screenshot({ path: `test-results/move-review-${viewport.width}-${theme}.png` });
      await dialog.getByRole('checkbox').check();
      await dialog.getByRole('button', { name: '确认移动并核验' }).click();
      await dialog.getByRole('button', { name: '核对撤回' }).click();
      await expect(dialog).toContainText('尚未执行本次撤回');
      await expect(dialog.getByRole('checkbox')).toBeInViewport({ ratio: 1 });
      await expect(scope).toContainText('本次撤回 2 张');
      await expect(scope.getByRole('status')).toContainText('等待确认撤回');
      await expect(dialog.getByText('尚未执行本次撤回。', { exact: false })).toBeInViewport({
        ratio: 1,
      });
      expect(await dialog.locator('.move-body').evaluate((el) => el.scrollTop)).toBe(0);
      await page.screenshot({
        path: `test-results/withdraw-review-${viewport.width}-${theme}.png`,
      });
    }
  }
});

test('safe organization keeps status text clear of both scrollbars', async ({ page }) => {
  await installMoveMock(page);
  await page.setViewportSize({ width: 960, height: 640 });
  for (const wideOverlay of [false, true]) {
    for (const theme of ['light', 'dark']) {
      await page.goto('/');
      await page.getByRole('button', { name: '体验示例图库' }).click();
      const toggle = page.getByLabel(theme === 'dark' ? '切换暗色主题' : '切换浅色主题');
      if (await toggle.isVisible()) await toggle.click();
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
      // A short list viewport represents a larger batch while keeping mock media bounded.
      await page.addStyleTag({ content: '.move-dialog .export-file-list { max-height: 100px; }' });
      await page.getByRole('button', { name: '安全整理', exact: true }).click();
      const dialog = page.getByRole('dialog', { name: '安全整理' });
      await dialog.getByRole('button', { name: '移动当前范围', exact: true }).click();
      await dialog.getByRole('button', { name: '选择整理位置' }).click();
      const status = dialog.locator('.export-file > span:last-child').first();
      await expect(status).toHaveText('待移动');
      await status.scrollIntoViewIfNeeded();
      const clearance = await status.evaluate((cell, overlay) => {
        const range = document.createRange();
        range.selectNodeContents(cell);
        const glyphs = [...range.getClientRects()].filter((r) => r.width > 0);
        return ['.move-body', '.export-file-list'].map((selector) => {
          const port = cell.closest(selector)!;
          const rect = port.getBoundingClientRect();
          const right = rect.left + port.clientLeft + port.clientWidth;
          let simulatedTrack = null;
          if (overlay) {
            // Model a 16px OS overlay explicitly: macOS headless Chrome may keep
            // offsetWidth === clientWidth even for custom scrollbar styles.
            const track = document.createElement('div');
            Object.assign(track.style, {
              position: 'fixed',
              left: `${right - 16}px`,
              top: `${rect.top + port.clientTop}px`,
              width: '16px',
              height: `${port.clientHeight}px`,
              borderRadius: '8px',
              background: '#7e887e',
              opacity: '.7',
              pointerEvents: 'none',
              zIndex: '99999',
            });
            track.dataset.qaScrollbar = 'simulated-overlay';
            document.body.append(track);
            const trackBox = track.getBoundingClientRect();
            simulatedTrack = {
              width: trackBox.width,
              overlapsText: glyphs.some((r) => r.right > trackBox.left + 0.5),
            };
          }
          return {
            selector,
            overflowing: port.scrollHeight > port.clientHeight,
            nativeTrackWidth: (port as HTMLElement).offsetWidth - port.clientWidth,
            textGap: right - Math.max(...glyphs.map((r) => r.right)),
            simulatedTrack,
          };
        });
      }, wideOverlay);
      expect(clearance.every((r) => r.overflowing)).toBe(true);
      if (wideOverlay)
        expect(
          clearance.every((r) => r.simulatedTrack?.width === 16 && !r.simulatedTrack.overlapsText),
          JSON.stringify(clearance),
        ).toBe(true);
      // Text needs visible breathing room even when an OS scroll thumb overlays content.
      expect(
        clearance.every((r) => r.textGap >= 12),
        JSON.stringify(clearance),
      ).toBe(true);
      await test.info().attach(`scrollport-${wideOverlay ? 'simulated' : 'native'}-${theme}`, {
        body: JSON.stringify(clearance),
        contentType: 'application/json',
      });
      await expect(dialog.getByRole('checkbox')).toBeInViewport({ ratio: 1 });
      await expect(dialog.getByRole('button', { name: '确认移动并核验' })).toBeDisabled();
      await page.screenshot({
        path: `test-results/scroll-clearance-${wideOverlay ? 'simulated-overlay' : 'native-overlay'}-${theme}.png`,
      });
    }
  }
});

test('watermark inputs protect ratings and fit both small-window themes', async ({ page }) => {
  await page.setViewportSize({ width: 960, height: 640 });
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  await page.getByRole('button', { name: '查看 山海_01.jpg', exact: true }).dblclick();
  const trigger = page.getByRole('button', { name: '日期城市水印', exact: true });
  await trigger.click();
  const dialog = page.getByRole('dialog', { name: '日期城市水印' });
  await expect(dialog).toContainText('浏览器仅布局示意');
  await dialog.getByLabel('手动城市').fill('温州');
  await page.keyboard.press('5');
  await expect(dialog.getByLabel('手动城市')).toHaveValue('温州5');
  await dialog.getByLabel('手动城市').fill('温州');
  await dialog.getByLabel('备用日期').fill('2026-10-25');
  await expect(dialog.locator('.watermark-illustration span')).toHaveText('2026-10-25 温州');
  await dialog.getByLabel('水印字体').selectOption('serif');
  await dialog.getByText('更多样式', { exact: true }).click();
  await dialog.getByLabel('水印透明度').fill('0.75');
  await expect(dialog.locator('output').nth(1)).toHaveText('75%');
  await expect(dialog.getByRole('button', { name: '选择导出位置' })).toBeDisabled();
  await expect(dialog.getByRole('button', { name: '确认生成并核验' })).toBeDisabled();
  await expect(dialog.getByRole('button', { name: '选择导出位置' })).toBeInViewport({ ratio: 1 });
  await expect(dialog.getByRole('button', { name: '确认生成并核验' })).toBeInViewport({ ratio: 1 });
  await dialog.getByLabel('手动城市').clear();
  await expect(dialog.locator('.watermark-illustration span')).toHaveText('2026-10-25');
  await dialog.getByLabel('手动城市').fill('温州');
  await dialog.getByText('更多样式', { exact: true }).click();
  await dialog.getByLabel('关闭水印').focus();
  await page.keyboard.press('Shift+Tab');
  await expect(dialog.locator('summary')).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(dialog.getByLabel('关闭水印')).toBeFocused();
  expect(
    await dialog.evaluate((el) => {
      const r = el.getBoundingClientRect();
      return (
        r.right <= innerWidth &&
        r.bottom <= innerHeight &&
        r.top >= 0 &&
        el.scrollWidth <= el.clientWidth
      );
    }),
  ).toBe(true);
  await page.screenshot({ path: 'test-results/watermark-light.png' });
  await page.keyboard.press('Escape');
  await expect(trigger).toBeFocused();
  await expect(page.locator('.single-stage').getByLabel('5 星')).not.toHaveClass(/rated/);
  await expect(page.locator('.single-stage .pane-name')).toContainText('山海_01.jpg');
  await page.getByLabel('切换暗色主题').click();
  await trigger.click();
  await dialog.getByLabel('备用日期').fill('2026-10-25');
  await dialog.getByLabel('手动城市').fill('温州');
  await page.screenshot({ path: 'test-results/watermark-dark.png' });
});

test('watermark batch freezes the explicit two-photo selection', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  for (const i of [1, 2])
    await page.getByRole('button', { name: `选择 山海_0${i}.jpg`, exact: true }).click();
  await page.getByRole('button', { name: '日期城市水印', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '日期城市水印' });
  await expect(dialog).toContainText('已选照片 · 2 张');
  await expect(dialog.getByLabel('预览照片').locator('option')).toHaveCount(2);
  await dialog.getByLabel('预览照片').selectOption('1');
  await expect(dialog.getByLabel('预览照片')).toHaveValue('1');
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.getByRole('button', { name: '选择 山海_02.jpg', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
});

test('logical picks export fixes the visible batch and isolates shortcuts in both themes', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 640 });
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  for (const i of [1, 2])
    await page.getByRole('button', { name: `选择 山海_0${i}.jpg`, exact: true }).click();
  await page.locator('.selection-bar').getByLabel('4 星', { exact: true }).click();
  await page.getByLabel('取消选择').click();
  await page.getByRole('button', { name: /家庭精选/ }).click();
  await expect(page.locator('.photo-card')).toHaveCount(2);
  await expect(page.locator('.collection-heading')).toContainText('不额外占用照片空间');
  const trigger = page.getByRole('button', { name: '导出照片副本', exact: true });
  await trigger.click();
  const dialog = page.getByRole('dialog', { name: '导出照片副本' });
  await expect(dialog).toContainText('当前筛选结果');
  await expect(dialog.locator('.export-file')).toHaveCount(2);
  await expect(dialog).toContainText('浏览器预览不复制文件');
  await expect(dialog.getByRole('button', { name: '选择导出位置' })).toBeDisabled();
  await page.keyboard.press('0');
  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('Tab');
  await expect(dialog.getByLabel('关闭导出副本')).toBeFocused();
  expect(
    await dialog.evaluate((el) => {
      const r = el.getBoundingClientRect();
      return r.right <= innerWidth && r.bottom <= innerHeight && r.top >= 0;
    }),
  ).toBe(true);
  await page.screenshot({ path: 'test-results/export-picks-light.png' });
  await page.keyboard.press('Escape');
  await expect(trigger).toBeFocused();
  await expect(page.locator('.photo-card')).toHaveCount(2);
  await page.getByLabel('切换暗色主题').click();
  await trigger.click();
  await expect(dialog).toBeVisible();
  await page.screenshot({ path: 'test-results/export-picks-dark.png' });
  await page.keyboard.press('Escape');
});

test('export selection contains only the explicitly selected photos', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  await page.getByRole('button', { name: '选择 山海_02.jpg', exact: true }).click();
  await page.getByRole('button', { name: '导出照片副本', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '导出照片副本' });
  await expect(dialog).toContainText('已选照片');
  await expect(dialog.locator('.export-file')).toHaveCount(1);
  await expect(dialog.locator('.export-file')).toContainText('山海_02.jpg');
  await page.keyboard.press('Escape');
});

test('relinking keeps review shortcuts isolated and explains browser limits', async ({ page }) => {
  await page.setViewportSize({ width: 960, height: 640 });
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  await page.getByRole('button', { name: '查看 山海_01.jpg', exact: true }).dblclick();
  await page.getByRole('button', { name: '重新关联目录', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '重新关联目录' });
  await expect(dialog).toContainText('浏览器预览不访问本机目录或修改资料库');
  await page.keyboard.press('0');
  await page.keyboard.press('ArrowRight');
  await expect(page.locator('.single-stage .pane-name')).toContainText('山海_01.jpg');
  await page.keyboard.press('Tab');
  await expect(dialog.getByLabel('关闭重新关联')).toBeFocused();
  await page.screenshot({ path: 'test-results/relink-preview-light.png' });
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole('button', { name: '重新关联目录', exact: true })).toBeFocused();
  await expect(page.locator('.single-stage').getByLabel('0 分：淘汰候选')).not.toHaveClass(/rated/);
  await page.getByLabel('切换暗色主题').click();
  await page.getByRole('button', { name: '重新关联目录', exact: true }).click();
  await expect(dialog).toBeVisible();
  await page.screenshot({ path: 'test-results/relink-preview-dark.png' });
  await page.keyboard.press('Escape');
  await expect(page.getByRole('button', { name: '重新关联目录', exact: true })).toBeFocused();
});

test('sync panel protects ratings and separates browser preview from XMP writes', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 640 });
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  await page.getByRole('button', { name: '查看 山海_01.jpg', exact: true }).dblclick();
  await page.getByRole('button', { name: '评分同步', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '评分同步' });
  await expect(dialog).toContainText('浏览器预览不读取或写入 XMP');
  await page.keyboard.press('0');
  await page.keyboard.press('ArrowRight');
  await expect(page.locator('.single-stage .pane-name')).toContainText('山海_01.jpg');
  await page.keyboard.press('Tab');
  await expect(dialog.getByLabel('关闭评分同步')).toBeFocused();
  await page.screenshot({ path: 'test-results/sync-preview-light.png' });
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole('button', { name: '评分同步', exact: true })).toBeFocused();
  await expect(page.locator('.single-stage').getByLabel('0 分：淘汰候选')).not.toHaveClass(/rated/);
});

test('seven-state review, stable advance, undo, comparison, and input safety', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.goto('/');
  await expect(page.getByRole('heading', { name: '给回忆，一点留白。' })).toBeVisible();
  await page.screenshot({ path: 'test-results/welcome-light.png' });
  await page.getByRole('button', { name: '体验示例图库' }).click();
  await expect(page.getByLabel('照片网格')).toBeVisible();
  await page.getByRole('button', { name: '未评分', exact: false }).first().click();
  await page.getByRole('button', { name: '查看 山海_01.jpg', exact: true }).dblclick();
  await page.keyboard.press('0');
  await expect(page.locator('.single-stage .pane-name')).toContainText('山海_02.jpg');
  await page.keyboard.press('Control+z');
  await expect(page.locator('.single-stage .pane-name')).toContainText('山海_01.jpg');
  await page.keyboard.press('5');
  await expect(page.locator('.single-stage .pane-name')).toContainText('山海_02.jpg');
  await page.keyboard.press('Escape');
  await page.getByLabel('评分筛选').selectOption('5');
  await expect(page.locator('.photo-card')).toHaveCount(1);
  await page.getByLabel('搜索照片').fill('0');
  await page.keyboard.press('1');
  await expect(page.getByLabel('搜索照片')).toHaveValue('01');
  await page.getByLabel('搜索照片').clear();
  await page.getByLabel('评分筛选').selectOption('all');
  await page.getByLabel('取消选择').click();
  // All eight are visible at this density; selecting does not open the viewer.
  for (let i = 1; i <= 8; i++)
    await page
      .getByRole('button', { name: `选择 山海_${String(i).padStart(2, '0')}.jpg`, exact: true })
      .click();
  await page.keyboard.press('c');
  await expect(page.locator('.photo-pane')).toHaveCount(8);
  await page.getByRole('region', { name: '比较照片 山海_03.jpg', exact: true }).click();
  await page.keyboard.press('4');
  await expect(
    page.getByRole('region', { name: '比较照片 山海_03.jpg', exact: true }).getByLabel('4 星'),
  ).toHaveClass(/rated/);
  await expect(
    page.getByRole('region', { name: '比较照片 山海_02.jpg', exact: true }).getByLabel('4 星'),
  ).not.toHaveClass(/rated/);
  await page.getByLabel('放大', { exact: true }).click();
  await expect(page.locator('.zoom-label')).toHaveText('125%');
  await page.screenshot({ path: 'test-results/compare-eight-light.png' });
  await page.keyboard.press('Space');
  await expect(page.locator('.photo-pane')).toHaveCount(1);
  await page.keyboard.press('Escape');
  await expect(page.locator('.photo-pane')).toHaveCount(8);
  await page.getByLabel('切换暗色主题').click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.screenshot({ path: 'test-results/compare-eight-dark.png' });
  await page.keyboard.press('Escape');
  await page.getByLabel('取消选择').click();
  await page.screenshot({ path: 'test-results/gallery-dark.png' });
  expect(errors).toEqual([]);
});

test('grid is virtualized and empty search and shortcuts remain usable', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  await expect(page.locator('.photo-card')).not.toHaveCount(24);
  await page.getByLabel('搜索照片').fill('不存在');
  await expect(page.getByRole('heading', { name: '没有符合条件的照片' })).toBeVisible();
  await page.getByLabel('搜索照片').clear();
  await page.getByRole('button', { name: '快捷键', exact: true }).click();
  await expect(page.getByRole('dialog', { name: '快捷键' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toHaveCount(0);
});

test('backup panel explains preview limits, traps focus, and protects review shortcuts', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 640 });
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  await page.getByRole('button', { name: '查看 山海_01.jpg', exact: true }).dblclick();
  await page.getByRole('button', { name: '资料库备份', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '资料库备份' });
  await expect(dialog).toContainText('浏览器预览不使用本机资料库');
  await page.keyboard.press('5');
  await page.keyboard.press('ArrowRight');
  await expect(page.locator('.single-stage .pane-name')).toContainText('山海_01.jpg');
  await page.keyboard.press('Tab');
  await expect(dialog.getByLabel('关闭资料库备份')).toBeFocused();
  await page.screenshot({ path: 'test-results/backup-light.png' });
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole('button', { name: '资料库备份', exact: true })).toBeFocused();
  await expect(page.locator('.single-stage').getByLabel('5 星')).not.toHaveClass(/rated/);
});

for (const count of [2, 3, 4, 6]) {
  test(`${count} pane comparison fits a small desktop and links zoom`, async ({ page }) => {
    await page.setViewportSize({ width: 960, height: 640 });
    await page.goto('/');
    await page.getByRole('button', { name: '体验示例图库' }).click();
    for (let i = 1; i <= count; i++) {
      const checkbox = page.getByRole('button', {
        name: `选择 山海_${String(i).padStart(2, '0')}.jpg`,
        exact: true,
      });
      await checkbox.click();
      await expect(checkbox).toHaveAttribute('aria-pressed', 'true');
    }
    await page.keyboard.press('c');
    await expect(page.locator('.photo-pane')).toHaveCount(count);
    await page.getByLabel('放大', { exact: true }).click();
    for (const image of await page.locator('.pane-canvas>img').all())
      await expect(image).toHaveCSS('transform', /1.25/);
    const fits = await page.locator('.photo-pane').evaluateAll((elements) =>
      elements.every((el) => {
        const r = el.getBoundingClientRect();
        return r.width > 100 && r.right <= window.innerWidth && r.bottom <= window.innerHeight;
      }),
    );
    expect(fits).toBe(true);
    await page.keyboard.press('Space');
    await expect(page.locator('.photo-pane')).toHaveCount(1);
    await page.keyboard.press('Escape');
    await expect(page.locator('.photo-pane')).toHaveCount(count);
  });
}

// Native API substitute only: drives adversarial IPC timing, not the filesystem.
async function installBatchMock(page: Page, kind: 'copy' | 'watermark' | 'move') {
  await page.addInitScript((kind) => {
    const svg = `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="48" height="32"><path fill="#617b68" d="M0 0h48v32H0z"/></svg>')}`;
    const assets = Array.from({ length: 100 }, (_, i) => ({
      id: `batch-${i}`,
      folderId: 1,
      name: `generated-${i}.jpg`,
      relative: `generated-${i}.jpg`,
      width: 4032,
      height: 3024,
      bytes: 4000000,
      modified: 1,
      captured: null,
      rating: 4,
      revision: 1,
      format: 'jpg',
    }));
    const options = {
      city: '温州',
      fallbackDate: '2026-11-10',
      font: 'sans',
      sizePercent: 2,
      color: '#FFFFFF',
      opacity: 0.88,
      marginPercent: 2.5,
      shadow: true,
    };
    const rows = assets.map((a, i) => ({
      name: a.relative,
      output: a.name,
      rating: a.rating,
      bytes: a.bytes,
      sidecar: false,
      status: i < 37 ? 'verified' : 'pending',
      error: null as string | null,
      caption: null,
    }));
    const base = {
      id: 'saved-batch',
      source: '/generated/sources',
      sourceFolder: 1,
      destination: '/generated/exports-parent/kept-batch',
      kind,
      watermark: kind === 'watermark' ? options : null,
      direction: 'forward',
      status: kind === 'move' ? 'paused' : 'cancelled',
      verified: 37,
      items: rows,
      bytes: 400000000,
      message: null,
      safetyBackup: null,
    };
    let recent = [base];
    const pending: { channel: { onmessage: (p: unknown) => void }; finish: () => void }[] = [];
    const controls = {
      cancels: 0,
      confirms: 0,
      ratings: 0,
      release: (n: number) => pending[n].finish(),
      late: (n: number) =>
        pending[n].channel.onmessage({
          phase: 'checking',
          checked: 99,
          total: 100,
          current: 'stale.jpg',
        }),
    };
    Object.assign(window, {
      isTauri: true,
      __batchControls: controls,
      __TAURI_INTERNALS__: {
        transformCallback: () => 1,
        unregisterCallback: () => {},
        convertFileSrc: () => svg,
        invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
          if (cmd === 'set_rating') {
            controls.ratings++;
            throw Error('Unexpected background rating');
          }
          if (cmd === 'application_info')
            return {
              version: '0.1.0',
              dataDirectory:
                '/generated/' +
                'long generated QA directory/'.repeat(9) +
                'generated-trial-catalog',
              isolated: true,
            };
          if (cmd === 'load_workspace') return null;
          if (cmd === 'save_workspace') return;
          if (cmd === 'create_demo') return { id: 1, name: 'Generated 100', demo: true };
          if (cmd === 'cancel_scan') return;
          if (cmd === 'scan_folder') {
            const ch = args.onEvent as { onmessage: (e: unknown) => void };
            ch.onmessage({ kind: 'batch', items: assets });
            ch.onmessage({ kind: 'done', count: 100, warnings: [] });
            return;
          }
          if (cmd === 'move_history' || cmd === 'export_history') return recent;
          if (cmd === 'watermark_preview')
            return {
              canExport: true,
              dataUrl: svg,
              info: {
                source: {
                  dateSource: 'manual',
                  date: '2026-11-10',
                  text: '2026-11-10 温州',
                  width: 4032,
                  height: 3024,
                  warning: null,
                },
                outputWidth: 4032,
                outputHeight: 3024,
              },
            };
          if (cmd === 'cancel_move' || cmd === 'cancel_export') {
            controls.cancels++;
            return;
          }
          if (cmd === 'choose_move_folder' || cmd === 'choose_export_folder') {
            const channel = args.onProgress as { onmessage: (p: unknown) => void };
            channel.onmessage({
              phase: 'checking',
              checked: 3,
              total: 100,
              current: 'generated-3.jpg',
            });
            await new Promise<void>((finish) => pending.push({ channel, finish }));
            return { ...base, status: 'ready', reviewToken: crypto.randomUUID() };
          }
          if (cmd === 'confirm_move' || cmd === 'confirm_export') {
            controls.confirms++;
            const result = {
              ...base,
              status: 'partial',
              reviewToken: null,
              items: rows.map((r, i) =>
                i === 37 ? { ...r, status: 'error', error: 'Generated occupied destination' } : r,
              ),
            };
            recent = [result];
            return result;
          }
          throw Error('Unexpected batch mock command ' + cmd);
        },
      },
    });
  }, kind);
}
for (const kind of ['copy', 'watermark', 'move'] as const) {
  test(`mock ${kind} batch review cancellation rejects late results and keeps 100-photo feedback usable`, async ({
    page,
  }) => {
    await installBatchMock(page, kind);
    await page.goto('/');
    await page.getByRole('button', { name: '体验示例图库' }).click();
    const label =
      kind === 'move' ? '安全整理' : kind === 'watermark' ? '日期城市水印' : '导出照片副本';
    await page.getByRole('button', { name: label, exact: true }).click();
    const dialog = page.getByRole('dialog', {
      name: kind === 'move' ? '安全整理' : kind === 'watermark' ? '日期城市水印' : '导出照片副本',
    });
    const resume = () =>
      dialog.getByRole('button', { name: kind === 'move' ? '继续' : '继续核验', exact: true });
    const confirm = () =>
      dialog.getByRole('button', {
        name:
          kind === 'move'
            ? '确认移动并核验'
            : kind === 'watermark'
              ? '确认生成并核验'
              : '确认复制并核验',
      });
    await resume().click();
    const preparing = dialog.getByRole('status', { name: '清单准备进度' });
    await expect(preparing).toContainText('3 / 100');
    await expect(preparing.getByRole('progressbar')).toHaveAttribute('value', '3');
    await expect(preparing).toBeInViewport({ ratio: 1 });
    await expect(dialog.getByRole('button', { name: '取消核对', exact: true })).toBeInViewport({
      ratio: 1,
    });
    await expect(confirm()).toBeDisabled();
    await page.screenshot({ path: `test-results/batch-${kind}-preparing-light.png` });
    await page.setViewportSize({ width: 960, height: 640 });
    await page.evaluate(() => {
      document.documentElement.dataset.theme = 'dark';
    });
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await expect(preparing).toBeInViewport({ ratio: 1 });
    await expect(dialog.getByRole('button', { name: '取消核对', exact: true })).toBeInViewport({
      ratio: 1,
    });
    await page.screenshot({ path: `test-results/batch-${kind}-preparing-small-dark.png` });
    await dialog.getByRole('button', { name: '取消核对', exact: true }).click();
    await expect(dialog.getByRole('button', { name: '正在取消核对…', exact: true })).toBeDisabled();
    await page.keyboard.press('Escape');
    await expect(dialog).toBeVisible();
    await page.evaluate(() => {
      (
        window as unknown as { __batchControls: { late: (n: number) => void } }
      ).__batchControls.late(0);
    });
    await expect(preparing.getByRole('progressbar')).toHaveAttribute('value', '3');
    await page.evaluate(() => {
      (
        window as unknown as { __batchControls: { release: (n: number) => void } }
      ).__batchControls.release(0);
    });
    await expect(preparing).toContainText('已取消清单核对');
    await expect(confirm()).toBeDisabled();
    await expect(dialog.getByRole('status', { name: '本次处理结果' })).toHaveCount(0);
    await resume().click();
    await page.evaluate(() => {
      (
        window as unknown as { __batchControls: { late: (n: number) => void } }
      ).__batchControls.late(0);
    });
    await expect(preparing).toContainText('3 / 100');
    await page.evaluate(() => {
      (
        window as unknown as { __batchControls: { release: (n: number) => void } }
      ).__batchControls.release(1);
    });
    await expect(preparing).toHaveCount(0);
    await expect(dialog.getByRole('progressbar')).toHaveCount(0);
    await expect(dialog.locator('.export-file-index svg')).toHaveCount(0);
    await expect(dialog).toContainText(
      kind === 'move' ? '尚未执行本次移动' : '尚未开始本次文件操作',
    );
    if (kind === 'move') await dialog.getByRole('checkbox').check();
    await expect(confirm()).toBeEnabled();
    await confirm().click();
    const outcome = dialog.getByRole('status', { name: '本次处理结果' });
    await expect(outcome).toContainText('37 / 100');
    await expect(outcome).toContainText('剩余 63');
    await expect(outcome).toContainText('1 张需处理');
    await page.screenshot({ path: `test-results/batch-${kind}-partial-small-dark.png` });
    expect(
      await page.evaluate(
        () =>
          (window as unknown as { __batchControls: { cancels: number } }).__batchControls.cancels,
      ),
    ).toBe(1);
    expect(
      await page.evaluate(
        () =>
          (window as unknown as { __batchControls: { confirms: number } }).__batchControls.confirms,
      ),
    ).toBe(1);
    expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
  });
}

test('local trial guide keeps data location readable and isolates keyboard focus', async ({
  page,
}) => {
  await installBatchMock(page, 'copy');
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库' }).click();
  const trigger = page.getByRole('button', { name: '使用说明与资料', exact: true });
  await trigger.click();
  const dialog = page.getByRole('dialog', { name: '使用说明与资料' });
  await expect(dialog).toContainText('0.1.0');
  await expect(dialog).toContainText('当前使用独立测试资料目录');
  await expect(dialog.locator('.guide-path')).toContainText('generated-trial-catalog');
  await page.keyboard.press('5');
  await page.keyboard.press('c');
  expect(
    await page.evaluate(
      () => (window as unknown as { __batchControls: { ratings: number } }).__batchControls.ratings,
    ),
  ).toBe(0);
  const close = dialog.getByRole('button', { name: '关闭使用说明' });
  await page.keyboard.press('Tab');
  await expect(close).toBeFocused();
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 640 });
    await page.evaluate((t) => {
      document.documentElement.dataset.theme = t;
    }, theme);
    await dialog.locator('.guide-body').evaluate((e) => {
      e.scrollTop = e.scrollHeight;
    });
    await expect(close).toBeInViewport();
    expect(await dialog.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);
    expect(
      await dialog.locator('.guide-body').evaluate((e) => e.scrollWidth <= e.clientWidth),
    ).toBe(true);
    await page.screenshot({ path: `../results/screenshots/trial-guide-${theme}.png` });
  }
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(trigger).toBeFocused();
});
