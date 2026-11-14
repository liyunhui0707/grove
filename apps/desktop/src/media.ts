import { useCallback, useEffect, useState } from 'react';
import type { Asset } from './domain';
import { mediaSource, native } from './bridge';

type Job = { signal: AbortSignal; run: () => Promise<void>; done: () => void };
const queue: Job[] = [];
let running = 0;
function drain() {
  while (running < 4 && queue.length) {
    const job = queue.shift()!;
    if (job.signal.aborted) {
      job.done();
      continue;
    }
    running++;
    void job.run().finally(() => {
      running--;
      job.done();
      drain();
    });
  }
}
function scheduled(signal: AbortSignal, run: () => Promise<void>) {
  return new Promise<void>((done) => {
    const job = { signal, run, done };
    queue.push(job);
    signal.addEventListener(
      'abort',
      () => {
        const index = queue.indexOf(job);
        if (index >= 0) {
          queue.splice(index, 1);
          done();
        }
      },
      { once: true },
    );
    drain();
  });
}
export function useMedia(asset: Asset, size: 480 | 1600 | 8192) {
  const [attempt, setAttempt] = useState(0);
  const source = mediaSource(asset, size, attempt);
  const request = `${source}\n${attempt}`;
  const retry = useCallback(() => setAttempt((n) => n + 1), []);
  const [result, setResult] = useState<{ source: string; url: string; error: string }>({
    source: '',
    url: '',
    error: '',
  });
  useEffect(() => {
    const controller = new AbortController();
    let url = '';
    if (!native) {
      setResult({ source: request, url: source, error: '' });
      return () => controller.abort();
    }
    void scheduled(controller.signal, async () => {
      try {
        const response = await fetch(source, { signal: controller.signal });
        if (!response.ok) throw new Error((await response.text()).slice(0, 200));
        const blob = await response.blob();
        if (controller.signal.aborted) return;
        url = URL.createObjectURL(blob);
        setResult({ source: request, url, error: '' });
      } catch (e) {
        if (!controller.signal.aborted)
          setResult({
            source: request,
            url: '',
            error: e instanceof Error ? e.message : String(e),
          });
      }
    });
    return () => {
      controller.abort();
      if (url) URL.revokeObjectURL(url);
    };
  }, [source, request]);
  return {
    ...(result.source === request ? result : { source: request, url: '', error: '' }),
    retry,
  };
}
