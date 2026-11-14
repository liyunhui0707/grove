import { useEffect, useRef, useState } from 'react';
import {
  Check,
  ChevronLeft,
  ChevronRight,
  FileCheck2,
  LoaderCircle,
  RefreshCw,
  ShieldCheck,
  X,
} from 'lucide-react';
import { bridge, native, type SyncReport, type SyncRow } from './bridge';
import { ratingLabel, type Batch, type Folder } from './domain';

export function SyncDialog({
  folder,
  onClose,
  onAdopted,
}: {
  folder: Folder;
  onClose: () => void;
  onAdopted: (batch: Batch) => void;
}) {
  const panel = useRef<HTMLElement>(null);
  const [report, setReport] = useState<SyncReport | null>(null);
  const [busy, setBusy] = useState(native);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [adopting, setAdopting] = useState<SyncRow | null>(null);
  useEffect(() => {
    const el = panel.current;
    if (el && !el.contains(document.activeElement))
      (el.querySelector<HTMLButtonElement>('button:not(:disabled)') ?? el).focus();
  }, [busy, adopting]);
  useEffect(() => {
    let cancelled = false;
    if (native)
      bridge
        .inspectXmp(folder.id)
        .then((value) => {
          if (!cancelled) setReport(value);
        })
        .catch((e) => {
          if (!cancelled) setError(String(e));
        })
        .finally(() => {
          if (!cancelled) setBusy(false);
        });
    return () => {
      cancelled = true;
    };
  }, [folder.id]);
  const refresh = async (offset = report?.offset ?? 0) => {
    setBusy(true);
    setError('');
    setAdopting(null);
    try {
      setReport(await bridge.inspectXmp(folder.id, offset));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const ready = report?.items.filter((row) => row.state === 'ready' || row.state === 'retry') ?? [];
  const sync = async () => {
    if (busy || !ready.length) return;
    setBusy(true);
    setError('');
    setNotice('');
    try {
      const result = await bridge.syncXmp(folder.id, ready);
      setReport(result.report);
      setNotice(
        `已核验 ${result.completed} 项${result.failed ? `，${result.failed} 项未完成，评分仍保存在本机` : result.report.pending ? `，仍有 ${result.report.pending} 项待处理` : '，评分已同步'}`,
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const adopt = async (row: SyncRow) => {
    if (busy || !row.reviewToken) return;
    setBusy(true);
    setError('');
    setNotice('');
    try {
      onAdopted(await bridge.adoptXmp(folder.id, row));
      setAdopting(null);
      setNotice('已采用文件中的评分，可在关闭面板后撤销。恢复现场继续保留。');
      setReport(await bridge.inspectXmp(folder.id));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const repair = async (row: SyncRow) => {
    if (busy) return;
    setBusy(true);
    setError('');
    setNotice('');
    try {
      await bridge.repairXmp(folder.id, row.id);
      setNotice('已恢复中断前的 XMP 文件，本机评分未改变。');
      setReport(await bridge.inspectXmp(folder.id));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div
      className="modal-backdrop"
      onClick={() => {
        if (!busy) onClose();
      }}
    >
      <section
        className="backup-dialog sync-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="评分同步"
        ref={panel}
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === 'Escape' && !busy) {
            e.preventDefault();
            if (adopting) setAdopting(null);
            else onClose();
          }
          if (e.key === 'Tab') {
            const buttons = Array.from(
              panel.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [],
            );
            const first = buttons[0],
              last = buttons.at(-1);
            if (!first) {
              e.preventDefault();
              panel.current?.focus();
            } else if (
              e.shiftKey &&
              (document.activeElement === first ||
                !buttons.includes(document.activeElement as HTMLButtonElement))
            ) {
              e.preventDefault();
              last?.focus();
            } else if (
              !e.shiftKey &&
              (document.activeElement === last ||
                !buttons.includes(document.activeElement as HTMLButtonElement))
            ) {
              e.preventDefault();
              first.focus();
            }
          }
        }}
      >
        <div className="dialog-heading">
          <FileCheck2 size={22} />
          <div>
            <span className="eyebrow">TAKE YOUR RATINGS WITH YOU</span>
            <h2>让评分，跟着照片走。</h2>
          </div>
          <button
            className="icon-button"
            aria-label="关闭评分同步"
            disabled={busy}
            onClick={onClose}
          >
            <X size={18} />
          </button>
        </div>
        <p className="backup-intro">
          {folder.demo ? '山海之间' : folder.name} · 评分已保存在本机，写入 sidecar
          后可随照片一起复制。
        </p>
        <div className="backup-scope">
          <ShieldCheck size={17} />
          <p>
            仅写独立的 XMP 文件，照片原件不变。已有 XMP
            会保留隐藏恢复副本；外部修改不会被静默覆盖。请先使用测试副本。
          </p>
        </div>
        {error && (
          <p className="backup-error" role="alert">
            {error}
          </p>
        )}
        {notice && (
          <p className="backup-notice" role="status">
            <Check size={14} />
            {notice}
          </p>
        )}
        {!native ? (
          <div className="backup-empty">请在桌面 App 中同步评分。浏览器预览不读取或写入 XMP。</div>
        ) : adopting ? (
          <div className="restore-confirm">
            <span className="eyebrow">USE THE FILE RATING</span>
            <h3 className="sync-confirm-name">{adopting.relative}</h3>
            <p>
              本机：{ratingLabel(adopting.rating)} → 文件：{ratingLabel(adopting.diskRating)}
            </p>
            <p>
              这会更新本机评分并接受当前文件为同步基线，不改写
              XMP。可撤销；如果文件再次变化，将停止本次操作。
            </p>
            <div className="backup-actions">
              <button className="button subtle" disabled={busy} onClick={() => setAdopting(null)}>
                返回列表
              </button>
              <button
                className="button primary"
                disabled={busy}
                onClick={() => void adopt(adopting)}
              >
                确认采用文件评分
              </button>
            </div>
          </div>
        ) : (
          <>
            <div className="backup-list-heading">
              <h3>
                待同步 <small>{report?.pending ?? '—'}</small>
              </h3>
              <button className="button subtle" disabled={busy} onClick={() => void refresh()}>
                <RefreshCw size={14} />
                重查待处理项
              </button>
            </div>
            <div className="sync-list">
              {report?.items.length ? (
                report.items.map((row) => (
                  <div className="sync-item" key={row.id}>
                    <div className="sync-item-top">
                      <strong title={row.relative}>{row.name}</strong>
                      <span className={`sync-state ${row.state}`}>
                        {
                          { ready: '可同步', conflict: '有冲突', retry: '可重试', error: '已暂停' }[
                            row.state
                          ]
                        }
                      </span>
                    </div>
                    {row.relative !== row.name && <p className="sync-message">{row.relative}</p>}
                    <p className="sync-ratings">
                      本机 {ratingLabel(row.rating)}
                      {row.diskKnown ? ` · 文件 ${ratingLabel(row.diskRating)}` : ''}
                    </p>
                    <p className="sync-message">{row.message}</p>
                    {row.reviewToken && (
                      <button
                        className="button subtle"
                        disabled={busy}
                        onClick={() => setAdopting(row)}
                      >
                        采用文件评分…
                      </button>
                    )}
                    {row.canRepair && (
                      <button
                        className="button subtle"
                        disabled={busy}
                        onClick={() => void repair(row)}
                      >
                        恢复中断前的 XMP
                      </button>
                    )}
                  </div>
                ))
              ) : (
                <div className="backup-empty">
                  {busy
                    ? '正在核对文件…'
                    : report
                      ? '评分已就绪，没有待同步项目。'
                      : '暂未取得同步状态，请重新检查。'}
                </div>
              )}
            </div>
            {report && (
              <div className="sync-pagination">
                <span>
                  {report.pending
                    ? `${report.offset + 1}–${report.offset + report.items.length} / ${report.pending}`
                    : '0 项待同步'}{' '}
                  · 恢复记录 {report.recoveries}
                </span>
                <div>
                  <button
                    className="icon-button"
                    aria-label="上一页同步项目"
                    disabled={busy || report.offset === 0}
                    onClick={() => void refresh(Math.max(0, report.offset - 100))}
                  >
                    <ChevronLeft size={16} />
                  </button>
                  <button
                    className="icon-button"
                    aria-label="下一页同步项目"
                    disabled={busy || report.offset + report.items.length >= report.pending}
                    onClick={() => void refresh(report.offset + 100)}
                  >
                    <ChevronRight size={16} />
                  </button>
                </div>
              </div>
            )}
            <p className="sync-footnote">
              每次最多核对本页 100 项；冲突项会保留。恢复副本位于原目录的隐藏 .moment-grove-*
              文件中，暂不自动清理。
            </p>
            <div className="backup-actions">
              <button
                className="button primary"
                disabled={busy || !ready.length || !report?.supported}
                onClick={() => void sync()}
              >
                <FileCheck2 size={15} />
                同步本页 {ready.length} 项
              </button>
            </div>
          </>
        )}
        {busy && (
          <p className="backup-busy" role="status">
            <LoaderCircle className="spin" size={14} />
            正在核验与处理，请稍候…
          </p>
        )}
      </section>
    </div>
  );
}
