import { test, expect } from '@playwright/test';

for (const total of [3000, 10000])
  test(`${total} photos virtualize and resume the exact rated occurrence`, async ({ page }) => {
    await page.addInitScript((total) => {
      const assets = Array.from({ length: total }, (_, i) => ({
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
          convertFileSrc: () =>
            `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="320" height="240"><path fill="#617b68" d="M0 0h320v240H0z"/></svg>')}`,
          invoke: async (cmd: string, args: Record<string, any> = {}) => {
            if (cmd === 'register_style_exit_guard') return;
            if (cmd === 'load_workspace')
              return JSON.parse(localStorage.getItem('workspace') || 'null');
            if (cmd === 'resume_workspace' || cmd === 'choose_folder')
              return { id: 1, name: `Generated ${total}`, demo: false };
            if (cmd === 'save_workspace') {
              localStorage.setItem(
                'workspace',
                JSON.stringify({ folderId: 1, name: `Generated ${total}`, view: args.view }),
              );
              return;
            }
            if (cmd === 'scan_folder') {
              for (let i = 0; i < assets.length; i += 64)
                args.onEvent.onmessage({ kind: 'batch', items: assets.slice(i, i + 64) });
              args.onEvent.onmessage({ kind: 'done', count: total, warnings: [] });
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
    }, total);
    await page.goto('/');
    await page
      .locator('.welcome')
      .getByRole('button', { name: '打开照片文件夹', exact: true })
      .click();
    await expect(
      page.getByRole('button', { name: `所有照片 ${total}`, exact: true }),
    ).toBeVisible();
    expect(await page.locator('.photo-card').count()).toBeLessThan(60);
    const grid = page.getByLabel('照片网格');
    await grid.evaluate((el) => {
      el.scrollTop = el.scrollHeight;
    });
    await expect(
      page.getByRole('button', {
        name: `查看 照片_${String(total - 1).padStart(5, '0')}.jpg`,
        exact: true,
      }),
    ).toBeVisible();
    expect(await page.locator('.photo-card').count()).toBeLessThan(60);
    await page.getByRole('textbox', { name: '搜索照片' }).fill(String(total - 1).padStart(5, '0'));
    await page
      .getByRole('button', {
        name: `查看 照片_${String(total - 1).padStart(5, '0')}.jpg`,
        exact: true,
      })
      .dblclick();
    await page.getByRole('checkbox', { name: '评分后下一张' }).uncheck();
    await page.locator('.pane-name').click();
    await page.keyboard.press('5');
    await expect(page.getByText('审片位置已保存', { exact: true })).toBeVisible();
    expect(await page.evaluate(() => localStorage.getItem('rated-ids'))).toBe(
      JSON.stringify([`photo-${total - 1}`]),
    );
    await page.reload();
    await expect(page.locator('.photo-pane')).toContainText(
      `照片_${String(total - 1).padStart(5, '0')}.jpg`,
    );
    await expect(
      page.locator('.photo-pane').getByRole('button', { name: '5 星', exact: true }),
    ).toHaveClass(/rated/);
    await expect(page.getByText('SDR 预览', { exact: true })).toBeVisible();
    const labelBox = await page.getByText('SDR 预览', { exact: true }).boundingBox();
    const modeBox = await page.locator('.view-mode-label').boundingBox();
    expect(Math.abs(labelBox!.y - modeBox!.y)).toBeLessThan(12);
    await page.keyboard.press('Escape');
    await page.getByRole('textbox', { name: '搜索照片' }).fill('');
    await expect(
      page.getByRole('button', { name: `所有照片 ${total}`, exact: true }),
    ).toBeVisible();
    expect(await page.locator('.photo-card').count()).toBeLessThan(60);
  });
