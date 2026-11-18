import { test, expect, type Page } from '@playwright/test';

async function workspaceMock(page: Page) {
  await page.addInitScript(() => {
    const read = (key: string) => JSON.parse(localStorage.getItem(key) || 'null');
    const assets = Array.from({ length: 6 }, (_, i) => ({
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
    Object.assign(window, {
      isTauri: true,
      __TAURI_INTERNALS__: {
        transformCallback: () => 1,
        unregisterCallback: () => {},
        convertFileSrc: () =>
          `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="48" height="32"><path fill="#617b68" d="M0 0h48v32H0z"/></svg>')}`,
        invoke: async (cmd: string, args: Record<string, any> = {}) => {
          if (cmd === 'register_style_exit_guard') return;
          if (cmd === 'load_workspace') return read('workspace');
          if (cmd === 'resume_workspace') {
            if (read('offline')) throw Error('上次文件夹已离线或身份变化，请重新选择文件夹');
            return { id: 1, name: 'Owned photos', demo: false };
          }
          if (cmd === 'save_workspace') {
            if (read('save-fails')) throw Error('无法保存审片位置');
            localStorage.setItem(
              'workspace',
              JSON.stringify({ folderId: args.folderId, name: 'Owned photos', view: args.view }),
            );
            return;
          }
          if (cmd === 'choose_folder') return { id: 1, name: 'Owned photos', demo: false };
          if (cmd === 'cancel_scan') return;
          if (cmd === 'scan_folder') {
            const items = read('missing-active')
              ? assets.filter((a) => a.id !== 'photo-2')
              : assets;
            args.onEvent.onmessage({ kind: 'batch', items });
            args.onEvent.onmessage({ kind: 'done', count: items.length, warnings: [] });
            return;
          }
          if (cmd === 'set_rating') {
            const changes = args.ids.map((id: string) => {
              const a = assets.find((a) => a.id === id)!;
              const before = a.rating;
              a.rating = args.rating;
              localStorage.setItem(`rating-${id.split('-')[1]}`, JSON.stringify(a.rating));
              return { id, before, after: a.rating, revision: 1 };
            });
            return { token: 'rating', changes };
          }
          if (cmd === 'application_info')
            return {
              version: '0.1.0',
              buildId: 'trial-test',
              channel: 'local-trial',
              dataDirectory: '/owned/runtime',
              isolated: true,
              executable: '/owned/installed/Grove.app/Contents/MacOS/moment-grove',
            };
          throw Error(`Unhandled ${cmd}`);
        },
      },
    });
  });
}

test('visible application identity opens exact app and catalog locations in a small dark window', async ({
  page,
}) => {
  await workspaceMock(page);
  await page.goto('/');
  const badge = page.getByRole('button', { name: '0.1.0 · 独立测试', exact: true });
  await expect(badge).toBeVisible();
  await expect(badge).toHaveAttribute('title', /trial-test/);
  await page.setViewportSize({ width: 960, height: 640 });
  await page.getByRole('button', { name: '切换暗色主题' }).click();
  await badge.click();
  const guide = page.getByRole('dialog', { name: '使用说明与资料' });
  await expect(guide).toContainText('/owned/runtime');
  await expect(guide).toContainText(
    '/owned/installed/Grove.app/Contents/MacOS/moment-grove',
  );
  await expect(guide).toContainText('同一资料目录重复打开会唤回已有窗口');
  await page.getByRole('button', { name: '关闭使用说明' }).click();
  await expect(badge).toBeFocused();
  await expect(badge).toBeInViewport();
});

test('relaunch resumes exact photo and preferences after rating', async ({ page }) => {
  await workspaceMock(page);
  await page.goto('/');
  await page
    .locator('.welcome')
    .getByRole('button', { name: '打开照片文件夹', exact: true })
    .click();
  await page.getByRole('button', { name: '查看 photo-2.jpg', exact: true }).dblclick();
  await page.getByRole('checkbox', { name: '评分后下一张' }).uncheck();
  await page.locator('.pane-name').click();
  await page.keyboard.press('4');
  await expect(page.locator('.photo-pane')).toContainText('photo-2.jpg');
  await expect(page.getByText('审片位置已保存', { exact: true })).toBeVisible();
  await page.reload();
  await expect(page.locator('.photo-pane')).toContainText('photo-2.jpg');
  await expect(page.getByRole('checkbox', { name: '评分后下一张' })).not.toBeChecked();
  await expect(
    page.locator('.photo-pane').getByRole('button', { name: '4 星', exact: true }),
  ).toHaveClass(/rated/);
});

test('filtered review resumes after active photo no longer matches live filter', async ({
  page,
}) => {
  await workspaceMock(page);
  await page.goto('/');
  await page
    .locator('.welcome')
    .getByRole('button', { name: '打开照片文件夹', exact: true })
    .click();
  await page.getByRole('button', { name: /^未评分/ }).click();
  await page.getByRole('button', { name: '查看 photo-2.jpg', exact: true }).dblclick();
  await page.getByRole('checkbox', { name: '评分后下一张' }).uncheck();
  await page.locator('.pane-name').click();
  await page.keyboard.press('5');
  await expect(page.getByText('审片位置已保存', { exact: true })).toBeVisible();
  await page.reload();
  await expect(page.locator('.photo-pane')).toContainText('photo-2.jpg');
  await page.getByRole('button', { name: '下一张', exact: true }).click();
  await expect(page.locator('.photo-pane')).toContainText('photo-3.jpg');
});

test('offline folder and missing photo never silently resume another occurrence', async ({
  page,
}) => {
  await workspaceMock(page);
  await page.goto('/');
  await page
    .locator('.welcome')
    .getByRole('button', { name: '打开照片文件夹', exact: true })
    .click();
  await page.getByRole('button', { name: '查看 photo-2.jpg', exact: true }).dblclick();
  await expect(page.getByText('审片位置已保存', { exact: true })).toBeVisible();
  await page.evaluate(() => localStorage.setItem('offline', 'true'));
  await page.reload();
  await expect(page.getByRole('alert')).toContainText('重新选择文件夹');
  await expect(page.locator('.photo-pane')).toHaveCount(0);
  await page.evaluate(() => {
    localStorage.removeItem('offline');
    localStorage.setItem('missing-active', 'true');
  });
  await page.reload();
  await expect(page.locator('.toast')).toContainText('上次照片已不在当前目录');
  await expect(page.locator('.photo-pane')).toHaveCount(0);
  await expect(page.locator('.photo-card')).toHaveCount(5);
});

test('workspace write failure is visible and does not claim saved', async ({ page }) => {
  await workspaceMock(page);
  await page.goto('/');
  await page.evaluate(() => localStorage.setItem('save-fails', 'true'));
  await page
    .locator('.welcome')
    .getByRole('button', { name: '打开照片文件夹', exact: true })
    .click();
  await expect(page.getByRole('alert')).toContainText('审片位置');
  await expect(page.getByText('审片位置已保存', { exact: true })).toHaveCount(0);
  await expect(page.locator('.photo-card')).toHaveCount(6);
});

test('grid search sort density and theme persist across relaunch', async ({ page }) => {
  await workspaceMock(page);
  await page.goto('/');
  await page
    .locator('.welcome')
    .getByRole('button', { name: '打开照片文件夹', exact: true })
    .click();
  await page.getByRole('textbox', { name: '搜索照片' }).fill('photo-');
  await page.getByRole('combobox', { name: '排序方式' }).selectOption('date');
  await page.getByRole('slider', { name: '缩略图大小' }).fill('340');
  await page.getByRole('button', { name: '切换暗色主题' }).click();
  await expect(page.getByText('审片位置已保存', { exact: true })).toBeVisible();
  await page.reload();
  await expect(page.getByRole('textbox', { name: '搜索照片' })).toHaveValue('photo-');
  await expect(page.getByRole('combobox', { name: '排序方式' })).toHaveValue('date');
  await expect(page.getByRole('slider', { name: '缩略图大小' })).toHaveValue('340');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.setViewportSize({ width: 960, height: 640 });
  await expect(page.getByText('审片位置已保存', { exact: true })).toBeInViewport();
  await page.screenshot({ path: 'test-results/workspace-grid-dark-small.png' });
});
