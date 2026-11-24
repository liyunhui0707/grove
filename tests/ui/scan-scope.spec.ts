import { test, expect } from '@playwright/test';

async function mixedFolder(page: import('@playwright/test').Page, photos: number) {
  await page.addInitScript(
    ({ photos }) => {
      const items = Array.from({ length: photos }, (_, i) => ({
        id: `scope-${i}`,
        folderId: 1,
        name: `sample-${i}.jpg`,
        relative: `sample-${i}.jpg`,
        width: 320,
        height: 240,
        bytes: 1000,
        modified: 0,
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
          convertFileSrc: () =>
            `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="320" height="240"><rect width="320" height="240" fill="#617b68"/></svg>')}`,
          invoke: async (cmd: string, args: Record<string, any> = {}) => {
            if (cmd === 'register_style_exit_guard') return;
            if (cmd === 'load_workspace') return null;
            if (cmd === 'choose_folder')
              return { id: 1, name: 'Generated mixed folder', demo: false };
            if (cmd === 'cancel_scan' || cmd === 'save_workspace') return;
            if (cmd === 'scan_folder') {
              if (items.length) args.onEvent.onmessage({ kind: 'batch', items });
              args.onEvent.onmessage({
                kind: 'done',
                count: items.length,
                warnings: [],
                skipped: { videos: 3, raw: 1, images: 2 },
              });
              return;
            }
            throw Error(`Unhandled ${cmd}`);
          },
        },
      });
    },
    { photos },
  );
  await page.goto('/');
  await page
    .locator('.welcome')
    .getByRole('button', { name: '打开照片文件夹', exact: true })
    .click();
}

test('mixed folders explain skipped media with whole-folder counts despite filtering', async ({
  page,
}) => {
  await mixedFolder(page, 2);
  await page.getByText('已跳过 6 个文件', { exact: true }).click();
  const scope = page.getByRole('note', { name: '扫描范围' });
  await expect(scope).toContainText('已载入 2 张照片');
  await expect(scope).toContainText('视频 3 个');
  await expect(scope).toContainText('RAW 1 个');
  await expect(scope).toContainText('不支持的图片 2 张');
  await page.getByRole('textbox', { name: '搜索照片' }).fill('no match');
  await expect(page.getByRole('heading', { name: '所有照片 0 / 2', exact: true })).toBeVisible();
  await expect(scope).toContainText('已载入 2 张照片');
});

test('a folder without supported photos explains media were skipped rather than lost', async ({
  page,
}) => {
  await mixedFolder(page, 0);
  await page.getByText('已跳过 6 个文件', { exact: true }).click();
  await expect(page.getByRole('note', { name: '扫描范围' })).toContainText('已载入 0 张照片');
  await expect(page.getByRole('note', { name: '扫描范围' })).toContainText('跳过的文件保留原位');
  await expect(page.getByRole('button', { name: '重新扫描当前文件夹', exact: true })).toBeVisible();
});

test('compact toolbar fits a narrow gallery while searching and exposes scan details', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 640 });
  await mixedFolder(page, 2);
  await page.getByLabel('搜索照片', { exact: true }).fill('nothing');
  const header = page.locator('.topbar');
  const reset = page.getByRole('button', { name: '清除筛选', exact: true });
  const search = page.locator('.search-field');
  expect((await reset.boundingBox())!.x + (await reset.boundingBox())!.width).toBeLessThanOrEqual(
    (await search.boundingBox())!.x,
  );
  await expect(header.getByLabel('导出照片副本')).toBeInViewport({ ratio: 1 });
  await reset.click();
  await expect(page.getByRole('heading', { name: '所有照片 2', exact: true })).toBeVisible();
  await expect(page.getByRole('note', { name: '扫描范围' })).toBeHidden();
  await page.getByText('已跳过 6 个文件', { exact: true }).click();
  await expect(page.getByRole('note', { name: '扫描范围' })).toBeInViewport({ ratio: 1 });
});
