import {
  dialogControls,
  TaskError,
  DirectoryPath,
  ExportTabs,
  ExportCompletion,
  exportHistoryLabel,
} from './TaskFeedback';
import { usePreparation, PreparationStatus, BatchOutcome } from './BatchFeedback';
import { useEffect, useRef, useState } from 'react';
import { Check, Copy, FolderOpen, LoaderCircle, RotateCcw, ShieldCheck, X } from 'lucide-react';
import { bridge, native, type ExportReport } from './bridge';
import { companionLabel, ratingLabel, type Asset, type Folder } from './domain';

const size = (bytes: number) =>
  bytes >= 1024 * 1024
    ? `${(bytes / 1024 / 1024).toFixed(1)} MiB`
    : `${Math.ceil(bytes / 1024)} KiB`;
const statusLabel = (status: string) =>
  ({
    ready: '待确认',
    running: '正在复制与核验',
    completed: '副本已核验',
    cancelled: '已取消，保留现有副本',
    partial: '部分完成，请检查结果',
  })[status] ?? status;

export function ExportDialog({
  folder,
  assets,
  scope,
  onClose,
  onSwitch,
}: {
  folder: Folder;
  assets: Asset[];
  scope: string;
  onClose: () => void;
  onSwitch: () => void;
}) {
  // Freeze the exact selection for this dialog. A later filter/scan cannot expand a confirmed batch.
  const [selection] = useState(() => assets.map((a) => ({ ...a })));
  const [report, setReport] = useState<ExportReport | null>(null);
  const [recent, setRecent] = useState<ExportReport[]>([]);
  const [busy, setBusy] = useState(false);
  const [running, setRunning] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [error, setError] = useState('');
  const [historyError, setHistoryError] = useState('');
  const [prepared, setPrepared] = useState(false);
  const [resuming, setResuming] = useState(false);
  const panel = useRef<HTMLElement>(null);
  const preparation = usePreparation(bridge.cancelExport);
  useEffect(() => {
    let stale = false;
    if (native)
      void bridge
        .exportHistory(folder.id)
        .then((r) => {
          if (!stale) setRecent(r.filter((item) => item.kind === 'copy'));
        })
        .catch((e) => {
          if (!stale) setHistoryError(String(e));
        });
    return () => {
      stale = true;
    };
  }, [folder.id]);
  useEffect(() => {
    if (panel.current && !panel.current.contains(document.activeElement))
      panel.current.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
  }, [busy]);
  const choose = async (resumeId: string | null = null) => {
    if (busy) return;
    const ticket = preparation.begin();
    setBusy(true);
    setError('');
    setPrepared(false);
    try {
      const result = await bridge.chooseExport(
        folder.id,
        resumeId ? [] : selection,
        resumeId,
        preparation.onProgress(ticket),
      );
      if (result && !preparation.ignored(ticket)) {
        setReport(result);
        setResuming(resumeId !== null);
        setPrepared(true);
      } else {
        if (!result) preparation.abandon(ticket);
        setReport(null);
      }
    } catch (e) {
      if (!preparation.ignored(ticket)) setError(String(e));
      else setReport(null);
    } finally {
      preparation.finish(ticket);
      setBusy(false);
    }
  };
  const start = async () => {
    if (!report || busy || !prepared) return;
    setBusy(true);
    setRunning(true);
    setCancelling(false);
    setError('');
    setPrepared(false);
    try {
      const result = await bridge.confirmExport(report.id, setReport);
      setReport(result);
      setRecent((await bridge.exportHistory(folder.id)).filter((item) => item.kind === 'copy'));
    } catch (e) {
      setError(String(e));
      void bridge
        .exportHistory(folder.id)
        .then((items) => setRecent(items.filter((item) => item.kind === 'copy')))
        .catch((e) => setHistoryError(String(e)));
    } finally {
      setBusy(false);
      setRunning(false);
      setCancelling(false);
    }
  };
  const cancel = async () => {
    setCancelling(true);
    try {
      await bridge.cancelExport();
    } catch (e) {
      setError(String(e));
      setCancelling(false);
    }
  };
  const rows =
    report?.items ??
    selection.map((a) => ({
      name: a.relative,
      output: '',
      rating: a.rating,
      bytes: a.bytes,
      status: 'pending',
      error: null,
    }));
  return (
    <div
      className="modal-backdrop"
      onClick={() => {
        if (!busy) onClose();
      }}
    >
      <section
        className="backup-dialog export-dialog"
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-label="导出照片副本"
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === 'Escape' && !busy) {
            e.preventDefault();
            onClose();
          }
          if (e.key === 'Tab') {
            const buttons = dialogControls(panel.current);
            const first = buttons[0],
              last = buttons.at(-1),
              active = document.activeElement as HTMLElement;
            if (!first) {
              e.preventDefault();
              panel.current?.focus();
            } else if (e.shiftKey && (active === first || !buttons.includes(active))) {
              e.preventDefault();
              last?.focus();
            } else if (!e.shiftKey && (active === last || !buttons.includes(active))) {
              e.preventDefault();
              first.focus();
            }
          }
        }}
      >
        <div className="dialog-heading">
          <Copy size={22} />
          <div>
            <h2>导出照片</h2>
          </div>
          <button
            className="icon-button"
            aria-label="关闭导出副本"
            disabled={busy}
            onClick={onClose}
          >
            <X size={18} />
          </button>
        </div>
        <ExportTabs disabled={busy} onSwitch={onSwitch} />
        <p className="dialog-intro">
          {resuming ? '上次导出清单' : scope} · {folder.name} ·{' '}
          {report?.items.length ?? selection.length} 张照片
        </p>
        {!resuming && selection.some((asset) => companionLabel(asset)) && (
          <p className="batch-review-note" role="note" aria-label="关联资源保护">
            当前选择含 Live Photo 标识或同名视频/RAW，暂不拆分复制。可改用水印生成静态
            JPEG；原件与关联资源保持原位。
          </p>
        )}
        <PreparationStatus task={preparation} preparing={busy && !running} />
        {prepared && <p className="batch-review-note">已准备好。点击下方按钮开始导出。</p>}
        <BatchOutcome
          report={report}
          reviewing={prepared || (busy && !running) || preparation.cancelled}
          action="复制"
        />
        <div className="export-body">
          <div className="backup-scope">
            <ShieldCheck size={17} />
            <p>复制原格式照片及评分文件，保留原始画质和元数据（含 GPS）。原件不变。</p>
          </div>
          {!native && (
            <p className="export-preview-note">
              浏览器预览不复制文件。请在桌面 App 中选择导出位置。
            </p>
          )}
          {selection.length > 100 && !report && (
            <p className="backup-error" role="alert">
              单次最多 100 张，请缩小选择后分批导出。
            </p>
          )}
          {error && <TaskError error={error} title="导出照片未完成" />}
          {report && <DirectoryPath label="导出到" path={report.destination} />}
          <div className="export-summary">
            <strong>
              {prepared
                ? '清单已核对，等待确认'
                : report
                  ? statusLabel(report.status)
                  : '这次带走的照片'}
            </strong>
            <span>
              {rows.length} 张 · {size(report?.bytes ?? selection.reduce((n, a) => n + a.bytes, 0))}
            </span>
          </div>
          <div className="export-file-list" aria-label="导出清单">
            {rows.length ? (
              rows.map((row, i) => (
                <div
                  className={`export-file ${row.status === 'error' ? 'has-error' : ''}`}
                  key={`${i}-${row.name}`}
                >
                  <span className="export-file-index">
                    {!prepared && !(busy && !running) && row.status === 'verified' ? (
                      <Check size={15} />
                    ) : (
                      String(i + 1).padStart(2, '0')
                    )}
                  </span>
                  <div>
                    <strong>{row.name}</strong>
                    <small>
                      {row.output ? `→ ${row.output}` : size(row.bytes)}
                      {row.error ? ` · ${row.error}` : ''}
                    </small>
                  </div>
                  <span>
                    {prepared || (busy && !running)
                      ? '待复制'
                      : row.status === 'verified'
                        ? '已核验'
                        : row.status === 'error'
                          ? '暂停'
                          : ratingLabel(row.rating)}
                  </span>
                </div>
              ))
            ) : (
              <p className="backup-empty">当前没有照片，可从下方继续未完成的导出。</p>
            )}
          </div>
          {report && report.status !== 'completed' && !prepared && !(busy && !running) && (
            <div className="export-progress" aria-live="polite">
              <progress max={report.items.length} value={report.verified} />
              <span>
                已核验 {report.verified} / {report.items.length} 张
              </span>
            </div>
          )}
          {report?.message && <p className="backup-error">{report.message}</p>}
          <details className="task-details">
            <summary>导出限制与中断恢复</summary>
            <p>
              单次最多 100
              张。取消或失败时保留已核验副本和部分文件，可重新选择原目标父目录后继续核验；同名不同内容不会覆盖。操作记录留在本机。
            </p>
          </details>
          {historyError && <p className="backup-error">历史记录：{historyError}</p>}
          {recent.length > 0 && (
            <div className="export-history">
              <h3>最近的导出</h3>
              {recent.map((r) => (
                <div className="export-history-row" key={r.id}>
                  <div>
                    <strong>{exportHistoryLabel(r.destination, r.kind, r.items.length)}</strong>
                    <small>
                      {statusLabel(r.status)} · {r.verified}/{r.items.length} 张
                    </small>
                  </div>
                  {r.status === 'completed' && (
                    <button
                      className="button subtle"
                      onClick={() =>
                        void bridge
                          .openExportFolder(folder.id, r.id)
                          .catch((e) => setError(String(e)))
                      }
                    >
                      打开文件夹
                    </button>
                  )}
                  {r.status !== 'completed' && (
                    <button
                      className="button subtle"
                      disabled={busy}
                      onClick={() => void choose(r.id)}
                    >
                      <RotateCcw size={14} />
                      继续核验
                    </button>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
        {report?.status === 'completed' && !prepared && !busy ? (
          <ExportCompletion
            folderId={folder.id}
            id={report.id}
            count={report.verified}
            onClose={onClose}
          />
        ) : (
          <div className="backup-actions export-actions">
            {busy && !running ? (
              <button
                className="button subtle"
                disabled={preparation.cancelling}
                onClick={() => void preparation.cancel().catch((e) => setError(String(e)))}
              >
                {preparation.cancelling ? '正在取消核对…' : '取消核对'}
              </button>
            ) : running ? (
              <button className="button subtle" disabled={cancelling} onClick={() => void cancel()}>
                {cancelling ? '正在停止…' : '停止并保留副本'}
              </button>
            ) : (
              <button
                className="button subtle"
                disabled={!native || busy || selection.length === 0 || selection.length > 100}
                onClick={() => void choose()}
              >
                <FolderOpen size={15} />
                {busy ? '正在核对…' : '选择导出位置'}
              </button>
            )}
            <button
              className="button primary"
              disabled={!native || !prepared || busy}
              onClick={() => void start()}
            >
              {busy ? <LoaderCircle size={15} className="spin" /> : <Copy size={15} />}开始导出
            </button>
          </div>
        )}
      </section>
    </div>
  );
}
