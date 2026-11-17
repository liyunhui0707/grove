import { describe, it, expect } from 'vitest';
import { createMediaQueue } from './media-queue';
const signal = () => new AbortController();
const tick = async () => {
  for (let i = 0; i < 8; i++) await Promise.resolve();
};
function hold() {
  let release!: () => void;
  const wait = new Promise<void>((r) => {
    release = r;
  });
  return { wait, release };
}
describe('bounded media scheduling', () => {
  it('serves visible work ahead of queued speculation without exceeding the limit', async () => {
    const schedule = createMediaQueue(1),
      a = hold(),
      order: string[] = [];
    const first = schedule(signal().signal, async () => {
      order.push('active');
      await a.wait;
    });
    const background = schedule(
      signal().signal,
      async () => {
        order.push('next');
      },
      'prefetch',
    );
    const visible = schedule(signal().signal, async () => {
      order.push('visible');
    });
    await tick();
    expect(order).toEqual(['active']);
    a.release();
    await Promise.all([first, background, visible]);
    expect(order).toEqual(['active', 'visible', 'next']);
  });
  it('allows only one prefetch while foreground work uses spare slots', async () => {
    const schedule = createMediaQueue(2),
      a = hold(),
      order: string[] = [];
    const first = schedule(
      signal().signal,
      async () => {
        order.push('next1');
        await a.wait;
      },
      'prefetch',
    );
    const second = schedule(
      signal().signal,
      async () => {
        order.push('next2');
      },
      'prefetch',
    );
    const foreground = schedule(signal().signal, async () => {
      order.push('visible');
    });
    await tick();
    expect(order).toEqual(['next1', 'visible']);
    a.release();
    await Promise.all([first, second, foreground]);
    expect(order).toEqual(['next1', 'visible', 'next2']);
  });
  it('drops canceled queued requests and recovers a slot after failed work', async () => {
    const schedule = createMediaQueue(1),
      a = hold(),
      canceled = signal(),
      order: string[] = [];
    const first = schedule(signal().signal, async () => {
      await a.wait;
      throw Error('offline');
    });
    const stale = schedule(
      canceled.signal,
      async () => {
        order.push('stale');
      },
      'prefetch',
    );
    canceled.abort();
    const active = schedule(signal().signal, async () => {
      order.push('active');
    });
    a.release();
    await Promise.all([first, stale, active]);
    expect(order).toEqual(['active']);
  });
});
