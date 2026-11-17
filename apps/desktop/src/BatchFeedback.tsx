import { useEffect, useRef, useState } from 'react';
import type { PreparationProgress } from './bridge';

// A cancelled review may still return a response or queued IPC events. Neither
// can revive its confirmation, including after a new review or unmount.
export function usePreparation(cancelNative: () => Promise<void>) {
  const sequence = useRef(0);
  const active = useRef(false);
  const requested = useRef(false);
  const [progress, setProgress] = useState<PreparationProgress | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const [cancelled, setCancelled] = useState(false);
  useEffect(
    () => () => {
      active.current = false;
      sequence.current++;
    },
    [],
  );
  const begin = () => {
    const id = ++sequence.current;
    active.current = true;
    requested.current = false;
    setCancelling(false);
    setCancelled(false);
    setProgress({ phase: 'selecting', checked: 0, total: 0, current: null });
    return id;
  };
  const ignored = (id: number) => id !== sequence.current || !active.current || requested.current;
  return {
    progress,
    cancelling,
    cancelled,
    begin,
    ignored,
    abandon: (id: number) => {
      if (id === sequence.current && active.current) requested.current = true;
    },
    onProgress: (id: number) => (p: PreparationProgress) => {
      if (!ignored(id)) setProgress(p);
    },
    finish: (id: number) => {
      if (id !== sequence.current || !active.current) return;
      active.current = false;
      setProgress(null);
      setCancelling(false);
      setCancelled(requested.current);
    },
    cancel: async () => {
      if (!active.current || requested.current) return;
      requested.current = true;
      setCancelling(true);
      await cancelNative();
    },
  };
}

export function PreparationStatus({
  task,
  preparing,
}: {
  task: ReturnType<typeof usePreparation>;
  preparing: boolean;
}) {
  if (!preparing && !task.cancelled) return null;
  const p = task.progress;
  return (
    <div className="batch-preparation" role="status" aria-label="清单准备进度">
      {preparing ? (
        <>
          <div>
            <strong>
              {task.cancelling
                ? '正在取消核对…'
                : p?.phase === 'rechecking'
                  ? '正在复核清单'
                  : '正在核对清单'}
            </strong>
            <span>{p?.total ? `${p.checked} / ${p.total} 张` : '等待选择位置'}</span>
          </div>
          <progress max={p?.total || 1} value={p?.total ? p.checked : undefined} />
          <small title={p?.current ?? undefined}>
            {p?.current || '仅核对清单，尚未开始本次文件操作。'}
          </small>
        </>
      ) : (
        <>
          <strong>已取消清单核对</strong>
          <small>本次未开始文件操作；请重新核对清单后再确认。</small>
        </>
      )}
    </div>
  );
}

export function BatchOutcome({
  report,
  reviewing,
  action,
}: {
  report: {
    status?: string;
    verified: number;
    items: { status: string; error: string | null }[];
  } | null;
  reviewing: boolean;
  action: string;
}) {
  if (
    !report ||
    reviewing ||
    report.status === 'completed' ||
    report.status === 'restored' ||
    report.status === 'running'
  )
    return null;
  const errors = report.items.filter((i) => i.error || i.status === 'error').length;
  return (
    <p className="batch-outcome" role="status" aria-label="本次处理结果">
      {action}核验 {report.verified} / {report.items.length} 张 · 剩余{' '}
      {Math.max(0, report.items.length - report.verified)} 张{errors > 0 && ` · ${errors} 张需处理`}
    </p>
  );
}
