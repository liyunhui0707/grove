import { useCallback, useContext, useEffect, useState } from 'react';
import { PreviewReport, previewProblem } from './preview-status';
import type { Asset } from './domain';
import { mediaSource, native } from './bridge';

import { createMediaQueue } from './media-queue';
const scheduled = createMediaQueue();

// Warm only the next screen-sized disk preview. Keep no Blob URL or decoded
// original; actual viewing still makes its normal grant/identity-checked request.
export function useReviewPrefetch(asset: Asset | undefined) {
  const source = asset ? mediaSource(asset, 1600) : '';
  useEffect(() => {
    if (!native || !source) return;
    const controller = new AbortController();
    const timer = setTimeout(() => {
      void scheduled(
        controller.signal,
        async () => {
          try {
            const response = await fetch(source, { signal: controller.signal });
            if (response.ok) await response.arrayBuffer();
          } catch {
            // Speculation never changes visible errors, navigation or ratings.
            // A real viewing request remains retryable after an offline/bad file.
          }
        },
        'prefetch',
      );
    }, 150);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [source]);
}

export function useMedia(asset: Asset, size: 480 | 1600 | 8192, enabled = true) {
  const report = useContext(PreviewReport);
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
    if (!enabled) {
      setResult({ source: '', url: '', error: '' });
      return;
    }
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
  }, [source, request, enabled]);
  const current =
    enabled && result.source === request ? result : { source: request, url: '', error: '' };
  useEffect(() => {
    if (current.error) report(asset.id, size, previewProblem(current.error).label);
  }, [asset.id, size, current.error, report]);
  const displayed = () => report(asset.id, size, '');
  const displayFailed = () => {
    if (!attempt && native) retry();
    else setResult({ source: request, url: '', error: '图像显示失败，可重试或继续查看其他照片' });
  };
  return {
    ...current,
    displayed,
    displayFailed,
    retry,
  };
}
