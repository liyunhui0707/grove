import { beforeEach, afterEach, expect, test, vi } from 'vitest';

const api = vi.hoisted(() => ({ loadWatermarkStyle: vi.fn(), saveWatermarkStyle: vi.fn() }));
vi.mock('./bridge', () => ({ native: true, bridge: api }));
beforeEach(() => {
  vi.resetModules();
  vi.useFakeTimers();
  api.loadWatermarkStyle.mockReset();
  api.saveWatermarkStyle.mockReset();
});
afterEach(() => {
  vi.useRealTimers();
});

test('presentation projection never carries batch city or date', async () => {
  const { styleFrom, defaultWatermarkOptions } = await import('./watermark-style');
  const style = styleFrom({
    ...defaultWatermarkOptions,
    city: 'test city',
    fallbackDate: '2026-11-15',
  });
  expect(Object.keys(style).sort()).toEqual([
    'color',
    'font',
    'marginPercent',
    'opacity',
    'shadow',
    'sizePercent',
  ]);
});

test('rapid style edits coalesce and reopening waits for the newest durable value', async () => {
  const { saveWatermarkStyle, loadWatermarkStyle, styleFrom, defaultWatermarkOptions } =
    await import('./watermark-style');
  let saved: unknown = null;
  api.saveWatermarkStyle.mockImplementation(async (style) => {
    saved = style;
  });
  api.loadWatermarkStyle.mockImplementation(async () => saved);
  const first = styleFrom({ ...defaultWatermarkOptions, sizePercent: 3 });
  const final = { ...first, sizePercent: 4 };
  const a = saveWatermarkStyle(first);
  const b = saveWatermarkStyle(final);
  const read = loadWatermarkStyle();
  await vi.advanceTimersByTimeAsync(400);
  await Promise.all([a, b]);
  expect(await read).toEqual(final);
  expect(api.saveWatermarkStyle).toHaveBeenCalledTimes(1);
  expect(api.saveWatermarkStyle).toHaveBeenCalledWith(final);
});

test('an older in-flight save cannot overwrite a newer edit after close and reopen', async () => {
  const { saveWatermarkStyle, loadWatermarkStyle, styleFrom, defaultWatermarkOptions } =
    await import('./watermark-style');
  let release!: () => void;
  let saved: unknown = null;
  api.saveWatermarkStyle
    .mockImplementationOnce(async (style) => {
      await new Promise<void>((r) => {
        release = r;
      });
      saved = style;
    })
    .mockImplementation(async (style) => {
      saved = style;
    });
  api.loadWatermarkStyle.mockImplementation(async () => saved);
  const first = styleFrom(defaultWatermarkOptions);
  const final = { ...first, font: 'serif' as const };
  const a = saveWatermarkStyle(first);
  await vi.advanceTimersByTimeAsync(200);
  const b = saveWatermarkStyle(final);
  const read = loadWatermarkStyle();
  expect(api.loadWatermarkStyle).not.toHaveBeenCalled();
  release();
  await vi.advanceTimersByTimeAsync(400);
  await Promise.all([a, b]);
  expect(await read).toEqual(final);
  expect(api.saveWatermarkStyle.mock.calls.map((c) => c[0])).toEqual([first, final]);
});

test('save failure is reported and does not poison an explicit retry', async () => {
  const { saveWatermarkStyle, styleFrom, defaultWatermarkOptions } =
    await import('./watermark-style');
  api.saveWatermarkStyle.mockRejectedValueOnce(Error('disk denied')).mockResolvedValue(undefined);
  const style = styleFrom(defaultWatermarkOptions);
  const failed = expect(saveWatermarkStyle(style)).rejects.toThrow('disk denied');
  await vi.advanceTimersByTimeAsync(200);
  await failed;
  const retry = saveWatermarkStyle(style);
  await vi.advanceTimersByTimeAsync(200);
  await retry;
  expect(api.saveWatermarkStyle).toHaveBeenCalledTimes(2);
});

test('quit waits for the debounce and actual in-flight write before completing', async () => {
  const { saveWatermarkStyle, flushWatermarkStyle, styleFrom, defaultWatermarkOptions } =
    await import('./watermark-style');
  let release!: () => void;
  api.saveWatermarkStyle.mockImplementation(
    () =>
      new Promise<void>((r) => {
        release = r;
      }),
  );
  const write = saveWatermarkStyle(styleFrom(defaultWatermarkOptions));
  let done = false;
  const quit = flushWatermarkStyle().then(() => {
    done = true;
  });
  await vi.advanceTimersByTimeAsync(199);
  expect(done).toBe(false);
  expect(api.saveWatermarkStyle).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1);
  expect(done).toBe(false);
  release();
  await Promise.all([write, quit]);
  expect(done).toBe(true);
});

test('quit retains a failed style even after dialog close, and explicit retry writes it', async () => {
  const { saveWatermarkStyle, flushWatermarkStyle, styleFrom, defaultWatermarkOptions } =
    await import('./watermark-style');
  api.saveWatermarkStyle.mockRejectedValueOnce(Error('disk denied')).mockResolvedValue(undefined);
  const final = styleFrom({ ...defaultWatermarkOptions, sizePercent: 4 });
  const write = expect(saveWatermarkStyle(final)).rejects.toThrow('disk denied');
  const quit = expect(flushWatermarkStyle()).rejects.toThrow('disk denied');
  await vi.advanceTimersByTimeAsync(200);
  await Promise.all([write, quit]);
  const retry = flushWatermarkStyle(true);
  await vi.advanceTimersByTimeAsync(200);
  await retry;
  expect(api.saveWatermarkStyle).toHaveBeenLastCalledWith(final);
  expect(api.saveWatermarkStyle).toHaveBeenCalledTimes(2);
});

test('quit follows newer edits and does not report an obsolete in-flight error', async () => {
  const { saveWatermarkStyle, flushWatermarkStyle, styleFrom, defaultWatermarkOptions } =
    await import('./watermark-style');
  let reject!: (error: Error) => void;
  api.saveWatermarkStyle
    .mockImplementationOnce(
      () =>
        new Promise<void>((_, r) => {
          reject = r;
        }),
    )
    .mockResolvedValue(undefined);
  const first = saveWatermarkStyle(styleFrom(defaultWatermarkOptions)).catch(() => {});
  await vi.advanceTimersByTimeAsync(200);
  const quit = flushWatermarkStyle();
  const final = { ...styleFrom(defaultWatermarkOptions), opacity: 0.5 };
  const second = saveWatermarkStyle(final);
  reject(Error('obsolete failed write'));
  await vi.advanceTimersByTimeAsync(400);
  await Promise.all([first, second, quit]);
  expect(api.saveWatermarkStyle).toHaveBeenLastCalledWith(final);
});
