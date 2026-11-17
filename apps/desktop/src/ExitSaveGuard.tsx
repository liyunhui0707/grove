import { useEffect, useRef, useState } from 'react';
import { LoaderCircle, ShieldCheck } from 'lucide-react';
import { bridge, native } from './bridge';
import { flushWatermarkStyle } from './watermark-style';

export function ExitSaveGuard({ flushReview }: { flushReview: (retry: boolean) => Promise<void> }) {
  const review = useRef(flushReview);
  review.current = flushReview;
  const dialog = useRef<HTMLDialogElement>(null);
  const request = useRef<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [discarding, setDiscarding] = useState(false);
  const [error, setError] = useState('');
  const [registrationError, setRegistrationError] = useState('');
  const [attempt, setAttempt] = useState(0);
  const showTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const prepare = async (token: number, retry = false) => {
    request.current = token;
    setDiscarding(false);
    setBusy(true);
    setError('');
    clearTimeout(showTimer.current);
    // A normal, already-saved quit should not flash a modal. Failures appear immediately.
    showTimer.current = setTimeout(() => {
      if (request.current === token) dialog.current?.showModal();
    }, 400);
    try {
      const results = await Promise.allSettled([review.current(retry), flushWatermarkStyle(retry)]);
      const errors = results.filter((r): r is PromiseRejectedResult => r.status === 'rejected');
      if (errors.length) throw Error(errors.map((r) => String(r.reason)).join('\n'));
      clearTimeout(showTimer.current);
      if (request.current === token) await bridge.resolveStyleExit(token, true);
    } catch (reason) {
      if (request.current === token) {
        clearTimeout(showTimer.current);
        dialog.current?.showModal();
        setError(`退出前保存未完成，已保留窗口：${String(reason)}`);
        setBusy(false);
      }
    }
  };
  useEffect(() => {
    if (!native) return;
    let stale = false;
    void bridge
      .registerStyleExitGuard((token) => {
        if (!stale) void prepare(token);
      })
      .then(() => {
        if (!stale) setRegistrationError('');
      })
      .catch(() => {
        if (!stale)
          setRegistrationError('退出保存保护未启用，请等待评分、审片位置和样式保存完成后再退出。');
      });
    return () => {
      stale = true;
      clearTimeout(showTimer.current);
    };
  }, [attempt]);
  const cancel = async () => {
    const token = request.current;
    if (token === null) return;
    request.current = null; // A late save must not acknowledge this canceled request.
    clearTimeout(showTimer.current);
    try {
      await bridge.resolveStyleExit(token, false);
      dialog.current?.close();
      setBusy(false);
    } catch (reason) {
      request.current = token;
      setError(`无法取消退出：${String(reason)}`);
      setBusy(false);
    }
  };
  const discard = async () => {
    const token = request.current;
    if (token === null) return;
    setDiscarding(true);
    setBusy(true);
    try {
      await bridge.resolveStyleExit(token, true);
    } catch (reason) {
      setError(`无法退出：${String(reason)}`);
      setBusy(false);
    }
  };
  return (
    <>
      {registrationError && (
        <div className="exit-registration-error" role="alert">
          {registrationError}
          <button onClick={() => setAttempt((n) => n + 1)}>重试启用退出保护</button>
        </div>
      )}
      <dialog
        ref={dialog}
        className="style-exit-dialog"
        aria-labelledby="style-exit-title"
        onCancel={(event) => {
          event.preventDefault();
          void cancel();
        }}
        onKeyDown={(event) => event.stopPropagation()}
      >
        <div className="dialog-heading">
          <ShieldCheck size={22} />
          <h2 id="style-exit-title">保存后退出</h2>
        </div>
        {busy ? (
          <p role="status">
            <LoaderCircle size={16} className="spin" />
            {discarding
              ? '正在退出，未保存的修改不会重试…'
              : '正在保存评分、审片位置和水印样式，完成后自动退出…'}
          </p>
        ) : (
          <p role="alert">{error}</p>
        )}
        <div className="style-exit-actions">
          <button onClick={() => void cancel()}>返回应用</button>
          {!busy && <button onClick={() => void discard()}>放弃未保存的修改并退出</button>}
          {!busy && (
            <button
              className="primary-button"
              onClick={() => request.current !== null && void prepare(request.current, true)}
            >
              重试保存并退出
            </button>
          )}
        </div>
      </dialog>
    </>
  );
}
