import { test, expect, type Page } from '@playwright/test';

test('Escape in a watermark select preserves the panel and current batch inputs', async ({
  page,
}) => {
  await mock(page);
  await open(page);
  await page.getByLabel('手动城市', { exact: true }).fill('QA City');
  const size = page.getByLabel('成品尺寸', { exact: true });
  await size.focus();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog', { name: '日期城市水印' })).toBeVisible();
  await expect(page.getByLabel('手动城市', { exact: true })).toHaveValue('QA City');
  await page.getByRole('button', { name: '关闭水印', exact: true }).focus();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog', { name: '日期城市水印' })).toHaveCount(0);
});

const style = {
  font: 'serif',
  sizePercent: 3.2,
  color: '#fedcba',
  opacity: 0.65,
  marginPercent: 4.2,
  shadow: false,
};
async function mock(
  page: Page,
  config: {
    style?: typeof style;
    holdLoad?: boolean;
    failLoad?: boolean;
    failSave?: boolean;
    history?: boolean;
    holdSave?: boolean;
    failRegistration?: boolean;
    failPreview?: boolean;
    previewWarning?: string;
    companions?: boolean;
  } = {},
) {
  await page.addInitScript((config) => {
    if (!localStorage.getItem('wm-initialized')) {
      localStorage.setItem('wm-initialized', '1');
      localStorage.setItem('wm-style', JSON.stringify(config.style ?? null));
    }
    const controls = {
      writes: [] as unknown[],
      failLoad: !!config.failLoad,
      failSave: !!config.failSave,
      releaseLoad: () => {},
      previews: [] as any[],
      plans: [] as any[],
      requestExit: (_token: number) => {},
      resolutions: [] as { token: number; proceed: boolean }[],
      releaseSave: () => {},
      holdSave: !!config.holdSave,
      failRegistration: !!config.failRegistration,
    };
    const asset = {
      id: 'generated-one',
      folderId: 1,
      name: 'generated.jpg',
      relative: 'generated.jpg',
      width: 4032,
      height: 3024,
      bytes: 100,
      modified: 1,
      captured: null,
      rating: 4,
      revision: 1,
      format: 'jpg',
      companions: { videos: config.companions ? 1 : 0, raw: 0, livePhoto: false },
    };
    const svg = `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="48" height="32"><path fill="#617b68" d="M0 0h48v32H0z"/></svg>')}`;
    const old = {
      id: 'old-batch',
      kind: 'watermark',
      source: '/generated',
      sourceFolder: 1,
      destination: '/generated/outputs/kept',
      watermark: {
        city: '旧批城市',
        fallbackDate: '2026-10-01',
        font: 'sans',
        sizePercent: 1.5,
        color: '#ffffff',
        opacity: 0.9,
        marginPercent: 1,
        shadow: true,
      },
      status: 'cancelled',
      verified: 0,
      bytes: 100,
      message: null,
      safetyBackup: null,
      items: [
        {
          name: 'generated.jpg',
          output: 'generated.jpg',
          rating: 4,
          bytes: 100,
          status: 'pending',
          error: null,
          caption: '2026-10-01 旧批城市',
        },
      ],
    };
    Object.assign(window, {
      isTauri: true,
      __wm: controls,
      __TAURI_INTERNALS__: {
        transformCallback: () => 1,
        unregisterCallback: () => {},
        convertFileSrc: () => svg,
        invoke: async (cmd: string, args: any = {}) => {
          if (cmd === 'register_style_exit_guard') {
            if (controls.failRegistration) throw Error('registration denied');
            controls.requestExit = (token) => args.onRequest.onmessage(token);
            return;
          }
          if (cmd === 'resolve_style_exit') {
            controls.resolutions.push({ token: args.token, proceed: args.proceed });
            return;
          }
          if (cmd === 'application_info')
            return {
              version: '0.4.0',
              buildId: 'style-test',
              channel: 'local-trial',
              isolated: true,
              dataDirectory: '/generated/runtime',
              executable: '/generated/App',
            };
          if (cmd === 'load_workspace') return null;
          if (cmd === 'save_workspace' || cmd === 'cancel_scan' || cmd === 'cancel_export') return;
          if (cmd === 'create_demo') return { id: 1, name: 'Generated', demo: true };
          if (cmd === 'scan_folder') {
            args.onEvent.onmessage({ kind: 'batch', items: [asset] });
            args.onEvent.onmessage({ kind: 'done', count: 1, warnings: [] });
            return;
          }
          if (cmd === 'load_watermark_style') {
            if (config.holdLoad)
              await new Promise<void>((r) => {
                controls.releaseLoad = r;
              });
            if (controls.failLoad) throw Error('invalid style record');
            return JSON.parse(localStorage.getItem('wm-style') || 'null');
          }
          if (cmd === 'save_watermark_style') {
            if (controls.holdSave)
              await new Promise<void>((resolve) => {
                controls.releaseSave = resolve;
              });
            if (controls.failSave) throw Error('disk denied');
            controls.writes.push(args.style);
            localStorage.setItem('wm-style', JSON.stringify(args.style));
            return;
          }
          if (cmd === 'export_history') return config.history ? [old] : [];
          if (cmd === 'watermark_preview') {
            controls.previews.push(args.options);
            if (config.failPreview) throw Error('current preview unavailable');
            return {
              dataUrl: svg,
              exportWidth: args.options.outputMaxEdge === 2048 ? 2048 : 4032,
              exportHeight: args.options.outputMaxEdge === 2048 ? 1536 : 3024,
              canExport: !!args.options.fallbackDate,
              info: {
                source: {
                  dateSource: args.options.fallbackDate ? 'manual' : 'missing',
                  date: args.options.fallbackDate,
                  text: args.options.fallbackDate
                    ? `${args.options.fallbackDate} ${args.options.city}`
                    : '',
                  width: 4032,
                  height: 3024,
                  warning: config.previewWarning ?? null,
                },
                outputWidth: 48,
                outputHeight: 32,
              },
            };
          }
          if (cmd === 'choose_export_folder') {
            controls.plans.push(args.watermark);
            return args.resumeId
              ? old
              : { ...old, id: 'new-batch', status: 'ready', watermark: args.watermark };
          }
          throw Error(`Unhandled ${cmd}`);
        },
      },
    });
  }, config);
}
async function open(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: '体验示例图库', exact: true }).click();
  await page.getByRole('button', { name: '资料库工具', exact: true }).click();
  await page.getByRole('button', { name: '日期城市水印', exact: true }).click();
  return page.getByRole('dialog', { name: '日期城市水印' });
}

test('paired still export explains its scope and does not label an unrelated historical batch', async ({
  page,
}) => {
  await mock(page, { companions: true, history: true });
  const dialog = await open(page);
  await expect(dialog.getByRole('note', { name: '静态导出范围' })).toContainText(
    '不是完整 Live Photo 导出',
  );
  await dialog.getByLabel('备用日期', { exact: true }).fill('2026-11-17');
  await expect(dialog.getByRole('button', { name: '选择导出位置', exact: true })).toBeEnabled();
  await dialog.getByRole('button', { name: '继续核验', exact: true }).click();
  await expect(dialog.getByRole('note', { name: '静态导出范围' })).toHaveCount(0);
});

test('normal quit waits for a style write even after closing its dialog', async ({ page }) => {
  await mock(page, { holdSave: true });
  const dialog = await open(page);
  await dialog.getByLabel('水印字体', { exact: true }).selectOption('serif');
  await dialog.getByRole('button', { name: '关闭水印', exact: true }).click();
  await page.evaluate(() => (window as any).__wm.requestExit(1));
  const exit = page.getByRole('dialog', { name: '保存后退出' });
  await expect(exit).toBeVisible();
  await expect(exit.getByRole('status')).toContainText('正在保存');
  expect(await page.evaluate(() => (window as any).__wm.resolutions)).toEqual([]);
  await page.waitForFunction(() => (window as any).__wm.releaseSave.toString() !== '() => {}');
  await page.evaluate(() => (window as any).__wm.releaseSave());
  await expect
    .poll(() => page.evaluate(() => (window as any).__wm.resolutions))
    .toEqual([{ token: 1, proceed: true }]);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('wm-style')!))).toMatchObject({
    font: 'serif',
  });
});

test('failed saving keeps the app open and a retry saves the latest style before exit', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 640 });
  await page.addInitScript(() => localStorage.setItem('moment-grove-theme', 'dark'));
  await mock(page, { failSave: true });
  const dialog = await open(page);
  await dialog.getByLabel('水印字体', { exact: true }).selectOption('serif');
  await page.evaluate(() => (window as any).__wm.requestExit(7));
  const exit = page.getByRole('dialog', { name: '保存后退出' });
  await expect(exit.getByRole('alert')).toContainText('已保留窗口');
  const bounds = await exit.boundingBox();
  expect(
    bounds &&
      bounds.x >= 0 &&
      bounds.y >= 0 &&
      bounds.x + bounds.width <= 960 &&
      bounds.y + bounds.height <= 640,
  ).toBe(true);
  await exit.screenshot({ path: '.local/exit-save-stage/dark-error-dialog.png' });
  expect(await page.evaluate(() => (window as any).__wm.resolutions)).toEqual([]);
  await page.evaluate(() => {
    (window as any).__wm.failSave = false;
  });
  await exit.getByRole('button', { name: '重试保存并退出' }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).__wm.resolutions))
    .toEqual([{ token: 7, proceed: true }]);
});

test('an explicit discard exits without claiming that the failed style was saved', async ({
  page,
}) => {
  await mock(page, { failSave: true });
  const dialog = await open(page);
  await dialog.getByLabel('水印字体').selectOption('serif');
  await page.evaluate(() => (window as any).__wm.requestExit(17));
  const exit = page.getByRole('dialog', { name: '保存后退出' });
  await expect(exit.getByRole('alert')).toContainText('已保留窗口');
  expect(await page.evaluate(() => (window as any).__wm.resolutions)).toEqual([]);
  await exit.getByRole('button', { name: '放弃未保存的修改并退出', exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).__wm.resolutions))
    .toEqual([{ token: 17, proceed: true }]);
  expect(await page.evaluate(() => localStorage.getItem('wm-style'))).toBe('null');
  expect(await page.evaluate(() => (window as any).__wm.writes)).toEqual([]);
});

test('returning to the app prevents a late write from completing the canceled quit', async ({
  page,
}) => {
  await mock(page, { holdSave: true });
  const dialog = await open(page);
  await dialog.getByLabel('水印字体', { exact: true }).selectOption('serif');
  await page.evaluate(() => (window as any).__wm.requestExit(11));
  const exit = page.getByRole('dialog', { name: '保存后退出' });
  await exit.getByRole('button', { name: '返回应用' }).click();
  await expect(exit).not.toBeVisible();
  await page.waitForFunction(() => (window as any).__wm.releaseSave.toString() !== '() => {}');
  await page.evaluate(() => (window as any).__wm.releaseSave());
  await expect(dialog.getByRole('status', { name: '水印样式保存状态' })).toHaveText('样式已保存');
  expect(await page.evaluate(() => (window as any).__wm.resolutions)).toEqual([
    { token: 11, proceed: false },
  ]);
  await page.evaluate(() => (window as any).__wm.requestExit(12));
  await expect
    .poll(() => page.evaluate(() => (window as any).__wm.resolutions))
    .toEqual([
      { token: 11, proceed: false },
      { token: 12, proceed: true },
    ]);
});

test('registration failure is visible and can be retried', async ({ page }) => {
  await mock(page, { failRegistration: true });
  await page.goto('/');
  await expect(page.getByRole('alert')).toContainText('退出保存保护未启用');
  await page.evaluate(() => {
    (window as any).__wm.failRegistration = false;
  });
  await page.getByRole('button', { name: '重试启用退出保护' }).click();
  await expect(page.getByText('退出保存保护未启用', { exact: false })).not.toBeVisible();
  await page.evaluate(() => (window as any).__wm.requestExit(15));
  await expect
    .poll(() => page.evaluate(() => (window as any).__wm.resolutions))
    .toEqual([{ token: 15, proceed: true }]);
});

test('styles survive close and reload, while every new batch clears city and fallback date', async ({
  page,
}) => {
  await mock(page);
  const dialog = await open(page);
  await dialog.getByLabel('水印字体').selectOption('serif');
  await dialog.getByLabel('水印字号').fill('3.2');
  await dialog.getByText('更多样式', { exact: true }).click();
  await dialog.getByLabel('水印颜色').fill('#fedcba');
  await dialog.getByLabel('水印透明度').fill('0.65');
  await dialog.getByLabel('水印边距').fill('4.2');
  await dialog.getByLabel('轻柔阴影').uncheck();
  await expect(dialog.getByRole('status', { name: '水印样式保存状态' })).toHaveText('样式已保存');
  const writes = await page.evaluate(() => (window as any).__wm.writes.length);
  await dialog.getByLabel('手动城市').fill('温州');
  await dialog.getByLabel('备用日期').fill('2026-11-15');
  await expect(dialog.getByRole('button', { name: '选择导出位置', exact: true })).toBeEnabled();
  expect(await page.evaluate(() => (window as any).__wm.writes.length)).toBe(writes);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('wm-style')!))).toEqual(style);
  await dialog.getByLabel('关闭水印').click();
  await page.getByRole('button', { name: '资料库工具', exact: true }).click();
  await page.getByRole('button', { name: '日期城市水印', exact: true }).click();
  await expect(dialog.getByLabel('水印字体')).toHaveValue('serif');
  await expect(dialog.getByLabel('手动城市')).toHaveValue('');
  await expect(dialog.getByLabel('备用日期')).toHaveValue('');
  await expect(dialog.getByRole('button', { name: '选择导出位置', exact: true })).toBeDisabled();
  await page.reload();
  await page.getByRole('button', { name: '体验示例图库', exact: true }).click();
  await page.getByRole('button', { name: '资料库工具', exact: true }).click();
  await page.getByRole('button', { name: '日期城市水印', exact: true }).click();
  await expect(dialog.getByLabel('水印字号')).toHaveValue('3.2');
  await expect(dialog.getByLabel('备用日期')).toHaveValue('');
  await dialog.getByLabel('手动城市').fill('本批城市');
  await dialog.getByLabel('备用日期').fill('2026-11-16');
  await dialog.getByRole('button', { name: '恢复默认样式' }).click();
  await expect(dialog.getByRole('status', { name: '水印样式保存状态' })).toHaveText('样式已保存');
  await expect(dialog.getByLabel('水印字体')).toHaveValue('sans');
  await expect(dialog.getByLabel('水印字号')).toHaveValue('2');
  await expect(dialog.getByLabel('手动城市')).toHaveValue('本批城市');
  await expect(dialog.getByLabel('备用日期')).toHaveValue('2026-11-16');
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('wm-style')!))).toEqual({
    font: 'sans',
    sizePercent: 2,
    color: '#ffffff',
    opacity: 0.88,
    marginPercent: 2.5,
    shadow: true,
  });
});

test('loading protects controls and history resumption never overwrites remembered style', async ({
  page,
}) => {
  await mock(page, { style, holdLoad: true, history: true });
  const dialog = await open(page);
  await expect(dialog.getByLabel('水印字体')).toBeDisabled();
  await expect(dialog.getByRole('button', { name: '继续核验' })).toBeDisabled();
  expect(await page.evaluate(() => (window as any).__wm.previews.length)).toBe(0);
  await page.evaluate(() => (window as any).__wm.releaseLoad());
  await expect(dialog.getByLabel('水印字体')).toHaveValue('serif');
  await dialog.getByRole('button', { name: '继续核验' }).click();
  await expect(dialog.getByLabel('水印字体')).toHaveValue('sans');
  await expect(dialog.getByLabel('手动城市')).toHaveValue('旧批城市');
  await expect(dialog.getByLabel('备用日期')).toHaveValue('2026-10-01');
  await expect(dialog.getByRole('status', { name: '水印样式保存状态' })).toContainText(
    '未改写已记住样式',
  );
  await expect(dialog.getByRole('button', { name: '恢复默认样式' })).toBeDisabled();
  expect(await page.evaluate(() => (window as any).__wm.writes.length)).toBe(0);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('wm-style')!))).toEqual(style);
});

test('load and save errors are visible, and retry recovers in a small dark dialog', async ({
  page,
}) => {
  await mock(page, { failLoad: true, failSave: true });
  await page.setViewportSize({ width: 960, height: 640 });
  const dialog = await open(page);
  await page.evaluate(() => (document.documentElement.dataset.theme = 'dark'));
  await expect(dialog.getByRole('alert')).toContainText('样式读取失败');
  await expect(dialog.getByLabel('水印字体')).toBeEnabled();
  await dialog.getByLabel('水印字体').selectOption('serif');
  await expect(dialog.getByRole('alert')).toContainText('样式未保存');
  await expect(dialog.getByRole('status', { name: '水印样式保存状态' })).not.toContainText(
    '样式已保存',
  );
  await page.evaluate(() => ((window as any).__wm.failSave = false));
  await dialog.getByRole('button', { name: '重试保存样式' }).click();
  await expect(dialog.getByRole('status', { name: '水印样式保存状态' })).toHaveText('样式已保存');
  await expect(dialog.getByRole('alert')).toHaveCount(0);
  await expect(dialog.getByRole('button', { name: '生成水印照片' })).toBeInViewport();
  expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
});

test('sharing size and quality reach preview and the frozen plan without changing remembered style', async ({
  page,
}) => {
  await mock(page);
  const dialog = await open(page);
  await dialog.getByLabel('备用日期').fill('2026-11-16');
  await dialog.getByLabel('成品尺寸', { exact: true }).selectOption('2048');
  await dialog.getByLabel('JPEG 质量', { exact: true }).selectOption('80');
  await expect(dialog.getByLabel('成品尺寸', { exact: true })).toHaveValue('2048');
  await expect(dialog.getByText('成品 2048 × 1536 px', { exact: false })).toBeVisible();
  await expect(dialog.getByRole('button', { name: '选择导出位置', exact: true })).toBeEnabled();
  expect(await page.evaluate(() => (window as any).__wm.previews.at(-1))).toMatchObject({
    outputMaxEdge: 2048,
    jpegQuality: 80,
  });
  expect(await page.evaluate(() => (window as any).__wm.writes)).toEqual([]);
  await dialog.getByRole('button', { name: '选择导出位置', exact: true }).click();
  await expect(dialog.getByLabel('本批导出设置')).toHaveText(
    '本批成品：长边至多 2048 px · JPEG 质量 80% · 不放大 · 参数已固定',
  );
  await expect(dialog.getByRole('button', { name: '生成水印照片', exact: true })).toBeEnabled();
  await dialog.getByLabel('JPEG 质量', { exact: true }).selectOption('98');
  await expect(dialog.getByLabel('本批导出设置')).toHaveCount(0);
  await expect(dialog.getByRole('button', { name: '生成水印照片', exact: true })).toBeDisabled();
  await dialog.getByLabel('关闭水印').click();
  await page.getByRole('button', { name: '资料库工具', exact: true }).click();
  await page.getByRole('button', { name: '日期城市水印', exact: true }).click();
  await expect(dialog.getByLabel('成品尺寸', { exact: true })).toHaveValue('original');
  await expect(dialog.getByLabel('JPEG 质量', { exact: true })).toHaveValue('92');
});

test('legacy batch resumes original-size 92 quality with controls fixed', async ({ page }) => {
  await mock(page, { history: true });
  const dialog = await open(page);
  await dialog.getByRole('button', { name: '继续核验' }).click();
  await expect(dialog.getByLabel('成品尺寸', { exact: true })).toHaveValue('original');
  await expect(dialog.getByLabel('JPEG 质量', { exact: true })).toHaveValue('92');
  await expect(dialog.getByLabel('成品尺寸', { exact: true })).toBeDisabled();
  await expect(dialog.getByLabel('本批导出设置')).toContainText('原尺寸 · JPEG 质量 92%');
});

for (const failPreview of [false, true]) {
  test(`historical batch hides unrelated current-photo ${failPreview ? 'preview error' : 'date warning'}`, async ({
    page,
  }) => {
    await mock(page, {
      history: true,
      failPreview,
      previewWarning: '当前照片没有有效 EXIF 日期',
    });
    const dialog = await open(page);
    const currentHint = dialog.getByText(
      failPreview ? 'current preview unavailable' : '当前照片没有有效 EXIF 日期',
      { exact: false },
    );
    await expect(currentHint.first()).toBeVisible();
    await dialog.getByRole('button', { name: '继续核验', exact: true }).click();
    await expect(dialog.getByText('使用上次保存的日期和样式。', { exact: false })).toBeVisible();
    await expect(dialog.getByLabel('手动城市', { exact: true })).toHaveValue('旧批城市');
    await expect(dialog.getByRole('button', { name: '生成水印照片', exact: true })).toBeEnabled();
    await expect(currentHint).toHaveCount(0);
    await expect(dialog.getByText('当前照片没有有效 EXIF 日期', { exact: true })).toHaveCount(0);
    await expect(dialog.getByLabel('本批导出设置')).toContainText('参数已固定');
    await dialog.getByRole('button', { name: '关闭水印', exact: true }).click();
    await page.getByRole('button', { name: '资料库工具', exact: true }).click();
    await page.getByRole('button', { name: '日期城市水印', exact: true }).click();
    await expect(currentHint.first()).toBeVisible();
  });
}
