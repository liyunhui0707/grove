import { describe, expect, it, vi } from 'vitest';
import { createWorkspaceWriter } from './workspace-writer';
import type { ReviewView } from './workspace';

const checkpoint = (active: string) => ({
  folderId: 1,
  view: { mode: 'review', active } as ReviewView,
});
describe('exit checkpoint drain', () => {
  it('waits for the in-flight write and coalesces to the latest immutable position', async () => {
    let release!: () => void;
    const persist = vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise<void>((r) => {
            release = r;
          }),
      )
      .mockResolvedValue(undefined);
    const writer = createWorkspaceWriter(persist);
    void writer.save(checkpoint('first'));
    void writer.save(checkpoint('skipped'));
    const latest = checkpoint('last');
    void writer.save(latest);
    latest.view.active = 'mutated';
    let exited = false;
    const flush = writer.flush().then(() => {
      exited = true;
    });
    await Promise.resolve();
    expect(exited).toBe(false);
    release();
    await flush;
    expect(persist.mock.calls.map(([value]) => value.view.active)).toEqual(['first', 'last']);
  });
  it('retains failure and retries only the checkpoint, never a rating operation', async () => {
    const persist = vi.fn().mockRejectedValueOnce(Error('denied')).mockResolvedValue(undefined);
    const writer = createWorkspaceWriter(persist);
    await expect(writer.save(checkpoint('retained'))).rejects.toThrow('denied');
    await expect(writer.flush()).rejects.toThrow('denied');
    await writer.flush(true);
    expect(persist).toHaveBeenCalledTimes(2);
    expect(persist.mock.calls[1][0].view.active).toBe('retained');
  });
  it('ignores an obsolete failure and follows a newer queued checkpoint', async () => {
    let reject!: (e: Error) => void;
    const persist = vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise<void>((_, r) => {
            reject = r;
          }),
      )
      .mockResolvedValue(undefined);
    const writer = createWorkspaceWriter(persist);
    void writer.save(checkpoint('old'));
    void writer.save(checkpoint('new'));
    const flush = writer.flush();
    reject(Error('old write denied'));
    await flush;
    expect(persist.mock.calls.map(([value]) => value.view.active)).toEqual(['old', 'new']);
  });
});
