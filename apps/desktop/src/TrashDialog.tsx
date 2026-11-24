import { useEffect, useRef, useState } from 'react';
import { LoaderCircle, Trash2, X, ShieldCheck } from 'lucide-react';
import { bridge, type TrashReport, type TrashReview } from './bridge';
import { dialogControls, DirectoryPath, TaskError } from './TaskFeedback';
import type { Asset, Folder } from './domain';

const status = (value: string) =>
  ({
    ready: '等待确认',
    running: '正在移到废纸篓',
    completed: '已移到废纸篓',
    stopped: '已停止，请核对结果',
    interrupted: '上次操作中断，请核对',
    pending: '未处理',
    requesting: '结果待核对',
    trashed: '已移到废纸篓',
    attention: '请核对关联文件',
  })[value] ?? value;
export function TrashDialog({
  folder,
  assets,
  onClose,
}: {
  folder: Folder;
  assets: Asset[];
  onClose: (changed: boolean) => void;
}) {
  const [selection] = useState(() => assets.map((a) => ({ ...a })));
  const [review, setReview] = useState<TrashReview | null>(null);
  const [report, setReport] = useState<TrashReport | null>(null);
  const [history, setHistory] = useState<TrashReport[]>([]);
  const [preparing, setPreparing] = useState(selection.length > 0);
  const [running, setRunning] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState('');
  const changed = useRef(false);
  const panel = useRef<HTMLElement>(null);
  const busy = preparing || running;
  useEffect(() => {
    panel.current?.querySelector<HTMLButtonElement>('button')?.focus();
    let stale = false;
    void (async () => {
      try {
        const recent = await bridge.trashHistory();
        if (stale) return;
        setHistory(recent);
        if (selection.length) {
          const result = await bridge.prepareTrash(folder.id, selection);
          if (!stale) {
            setReview(result);
            setReport(result.report);
          }
        }
      } catch (e) {
        if (!stale) setError(String(e));
      } finally {
        if (!stale) setPreparing(false);
      }
    })();
    return () => {
      stale = true;
      void bridge.cancelTrash().catch(() => {});
    };
  }, [folder.id, selection]);
  const close = () => {
    if (!running) onClose(changed.current);
  };
  const start = async () => {
    if (!review || busy) return;
    const token = review.token;
    setReview(null);
    setRunning(true);
    setError('');
    changed.current = true;
    try {
      setReport(await bridge.confirmTrash(token, setReport));
    } catch (e) {
      setError(String(e));
    } finally {
      try {
        setHistory(await bridge.trashHistory());
      } catch (e) {
        setError((prev) => prev || String(e));
      }
      setRunning(false);
      setStopping(false);
    }
  };
  const stop = async () => {
    setStopping(true);
    try {
      await bridge.cancelTrash();
    } catch (e) {
      setError(String(e));
      setStopping(false);
    }
  };
  const rows = (r: TrashReport) => (
    <ul className="trash-files">
      {r.items.map((item, n) => (
        <li key={n}>
          <div>
            <strong>{item.name}</strong>
            <span>{status(item.status)}</span>
          </div>
          <small>{item.resources.length > 1 ? '含关联 XMP' : '仅照片，无关联 XMP'}</small>
          {r.status !== 'ready' && (
            <details>
              <summary>文件结果与位置</summary>
              {item.resources.map((file, k) => (
                <p key={k}>
                  <strong>{file.name}</strong> · {status(file.status)}
                  {file.destination && <code>{file.destination}</code>}
                </p>
              ))}
            </details>
          )}
        </li>
      ))}
    </ul>
  );
  return (
    <div className="modal-backdrop" onClick={close}>
      <section
        className="backup-dialog move-dialog trash-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="移到废纸篓"
        tabIndex={-1}
        ref={panel}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === 'Escape') {
            e.preventDefault();
            close();
          }
          if (e.key === 'Tab') {
            const controls = dialogControls(panel.current),
              active = document.activeElement as HTMLElement;
            if (!controls.length) {
              e.preventDefault();
              panel.current?.focus();
            } else if (e.shiftKey && (active === controls[0] || !controls.includes(active))) {
              e.preventDefault();
              controls.at(-1)?.focus();
            } else if (!e.shiftKey && (active === controls.at(-1) || !controls.includes(active))) {
              e.preventDefault();
              controls[0].focus();
            }
          }
        }}
      >
        <div className="dialog-heading">
          <Trash2 size={22} />
          <h2>{selection.length ? '移到废纸篓' : '废纸篓操作记录'}</h2>
          <button
            className="icon-button"
            aria-label="关闭废纸篓操作"
            disabled={running}
            onClick={close}
          >
            <X size={18} />
          </button>
        </div>
        <p className="dialog-intro">
          实际文件将离开原文件夹。每批最多 100 张，可从系统废纸篓找回。
        </p>
        {selection.length > 0 && (
          <div className="move-scope" role="status">
            <div className="move-scope-heading">
              <strong>仅处理已选的 {selection.length} 张 0 分照片</strong>
              <span>{preparing ? '正在核对文件…' : report ? status(report.status) : '未开始'}</span>
            </div>
            <span>来源：{folder.name}</span>
            {report && report.status !== 'ready' && (
              <p>
                {report.completed} / {report.items.length} 张照片已完成（含关联 XMP）
              </p>
            )}
          </div>
        )}
        <div className="move-body">
          {error && (
            <TaskError
              error={error}
              title="废纸篓操作未完成"
              hint="请核对详细原因、原目录和废纸篓。不会自动重试；文件结果以操作记录为准。"
            />
          )}
          {preparing && (
            <p role="status">
              <LoaderCircle size={16} className="spin" />{' '}
              正在核对评分、文件身份与关联资源；尚未移动文件。
            </p>
          )}
          {report && (
            <>
              <DirectoryPath label="原照片文件夹" path={report.source} />
              {rows(report)}
              {report.message && (
                <p className="trash-warning" role="alert">
                  {report.message}
                </p>
              )}
            </>
          )}
          <p className="trash-recovery">
            <ShieldCheck size={16} /> 可在 Finder 的废纸篓中找回文件。照片和 XMP
            都需按清单原名放回原文件夹，再重新扫描。若原位置已有同名文件，请取消替换并先核对。
          </p>
          {history.length > 0 && (
            <details className="trash-history">
              <summary>最近操作记录（{history.length}）</summary>
              <p>记录显示操作当时的结果。仅找回照片时，原有 XMP 仍需单独找回。</p>
              {history.map((r) => (
                <details key={r.id}>
                  <summary>
                    {new Date(r.created * 1000).toLocaleString('zh-CN')} · {r.completed}/
                    {r.items.length} 张 · {status(r.status)}
                  </summary>
                  {r.message && <p role="alert">{r.message}</p>}
                  {rows(r)}
                </details>
              ))}
            </details>
          )}
          {!selection.length && !history.length && !error && (
            <p>暂无记录。先在淘汰候选中选择照片，再移到废纸篓。</p>
          )}
        </div>
        <div className="move-actions">
          <button className="button subtle" onClick={close} disabled={running}>
            {changed.current ? '完成并刷新照片' : '取消'}
          </button>
          {running ? (
            <button className="button subtle" disabled={stopping} onClick={() => void stop()}>
              {stopping ? '等待当前照片处理完成…' : '停止后续处理'}
            </button>
          ) : (
            selection.length > 0 &&
            !changed.current && (
              <button
                className="button trash-action"
                disabled={!review || busy}
                onClick={() => void start()}
              >
                <Trash2 size={16} />
                确认移到废纸篓 · {selection.length} 张
              </button>
            )
          )}
        </div>
      </section>
    </div>
  );
}
