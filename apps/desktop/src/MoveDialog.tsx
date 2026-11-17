import { dialogControls, TaskError, DirectoryPath } from './TaskFeedback';
import { usePreparation, PreparationStatus, BatchOutcome } from './BatchFeedback';
import { useEffect, useRef, useState } from 'react';
import {
  Check,
  FolderInput,
  FolderOpen,
  LoaderCircle,
  RotateCcw,
  ShieldCheck,
  X,
} from 'lucide-react';
import { bridge, native, type MoveReport } from './bridge';
import { ratingLabel, type Asset, type Folder } from './domain';

const label = (s: string, direction = 'forward') =>
  ({
    ready: '清单已核对，等待确认',
    running: direction === 'restore' ? '正在撤回与核验' : '正在移动与核验',
    completed: '移动已核验',
    restored: '已撤回原位置',
    paused: direction === 'restore' ? '已停止，等待继续撤回' : '已停止，等待继续或撤回',
    partial: '操作暂停，请核对后恢复',
    planned: '等待恢复',
  })[s] ?? s;
export function MoveDialog({
  folder,
  assets,
  scope,
  onClose,
}: {
  folder: Folder | null;
  assets: Asset[];
  scope: string;
  onClose: (source?: Folder) => void;
}) {
  const [selection] = useState(() => assets.map((a) => ({ ...a })));
  const [kind, setKind] = useState<'move' | 'quarantine'>('quarantine');
  const [report, setReport] = useState<MoveReport | null>(null);
  const [recent, setRecent] = useState<MoveReport[]>([]);
  const [busy, setBusy] = useState(false);
  const [running, setRunning] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [prepared, setPrepared] = useState(false);
  const [acknowledged, setAcknowledged] = useState(false);
  const [error, setError] = useState('');
  const [changed, setChanged] = useState<Folder>();
  const panel = useRef<HTMLElement>(null);
  const preparation = usePreparation(bridge.cancelMove);
  const body = useRef<HTMLDivElement>(null);
  const eligible =
    kind === 'quarantine' ? selection.filter((a) => a.rating === 0 || a.rating === 1) : selection;
  const close = () => {
    if (!busy) onClose(changed);
  };
  useEffect(() => {
    panel.current?.querySelector<HTMLButtonElement>('button')?.focus();
    let stale = false;
    if (native)
      void bridge
        .moveHistory()
        .then((items) => {
          if (!stale) setRecent(items);
        })
        .catch((e) => {
          if (!stale) setError(String(e));
        });
    return () => {
      stale = true;
    };
  }, []);
  useEffect(() => {
    if (report?.reviewToken) body.current?.scrollTo({ top: 0 });
  }, [report?.reviewToken]);
  const refresh = async () => setRecent(await bridge.moveHistory());
  const choose = async (id: string | null = null, restore = false) => {
    if (busy) return;
    const ticket = preparation.begin();
    setBusy(true);
    setError('');
    setPrepared(false);
    setAcknowledged(false);
    try {
      const result = await bridge.chooseMove(
        folder?.id ?? null,
        id ? [] : eligible,
        kind,
        id,
        restore,
        preparation.onProgress(ticket),
      );
      if (result && !preparation.ignored(ticket)) {
        setKind(result.kind);
        setReport(result);
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
    if (!report?.reviewToken || !prepared || !acknowledged || busy) return;
    setBusy(true);
    setRunning(true);
    setPrepared(false);
    setError('');
    const source = {
      id: report.sourceFolder,
      name: report.source.split(/[\\/]/).at(-1) || '照片',
      demo: false,
    };
    // Even an interrupted command can have changed positions. Refresh only after the dialog closes.
    setChanged(source);
    try {
      setReport(await bridge.confirmMove(report.reviewToken, setReport));
      await refresh();
    } catch (e) {
      setError(String(e));
      try {
        await refresh();
      } catch {
        /* Preserve first error. */
      }
    } finally {
      setBusy(false);
      setRunning(false);
      setStopping(false);
    }
  };
  const stop = async () => {
    setStopping(true);
    try {
      await bridge.cancelMove();
    } catch (e) {
      setError(String(e));
      setStopping(false);
    }
  };
  const rows =
    report?.items ??
    eligible.map((a) => ({
      name: a.relative,
      output: a.name,
      rating: a.rating,
      sidecar: false,
      status: 'pending',
      error: null,
    }));
  const active = recent.some((r) => !['completed', 'restored'].includes(r.status));
  // A review can retain verified rows from an earlier operation. They are not
  // progress for the action the user is about to confirm.
  const reviewing = Boolean(report?.reviewToken);
  const preparing = busy && !running;
  const action = report?.direction === 'restore' ? '撤回' : '移动';
  const scopeAction =
    report?.direction === 'restore'
      ? '撤回'
      : (report?.kind ?? kind) === 'quarantine'
        ? '隔离'
        : '移动';
  const sourceName = report
    ? report.source.split(/[\\/]/).at(-1) || '照片'
    : (folder?.name ?? '操作历史与恢复');
  const statusTitle = error
    ? '请查看提示，重新核对'
    : preparing
      ? '正在核对清单'
      : running
        ? label('running', report?.direction)
        : reviewing
          ? prepared
            ? `清单已核对，等待确认${action}`
            : '请重新核对清单'
          : report
            ? label(report.status, report.direction)
            : kind === 'quarantine'
              ? '这次暂时放下的照片'
              : '这次要移动的照片';
  return (
    <div className="modal-backdrop" onClick={close}>
      <section
        className="backup-dialog move-dialog"
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-label="安全整理"
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === 'Escape' && !busy) {
            e.preventDefault();
            close();
          }
          if (e.key === 'Tab') {
            const controls = dialogControls(panel.current);
            const a = document.activeElement as HTMLElement;
            if (!controls.length) {
              e.preventDefault();
              panel.current?.focus();
            } else if (e.shiftKey && (a === controls[0] || !controls.includes(a))) {
              e.preventDefault();
              controls.at(-1)?.focus();
            } else if (!e.shiftKey && (a === controls.at(-1) || !controls.includes(a))) {
              e.preventDefault();
              controls[0].focus();
            }
          }
        }}
      >
        <div className="dialog-heading">
          <FolderInput size={22} />
          <div>
            <h2>移动与隔离</h2>
          </div>
          <button className="icon-button" aria-label="关闭安全整理" disabled={busy} onClick={close}>
            <X size={18} />
          </button>
        </div>
        <p className="dialog-intro">会移动实际文件 · 仅支持同一磁盘，可核对撤回</p>
        <div className="move-scope" role="region" aria-label="本次整理范围">
          <div className="move-scope-heading">
            <strong>
              {folder || report ? `本次${scopeAction} ${rows.length} 张` : '从历史继续或撤回'}
            </strong>
            {report && <span role="status">{statusTitle}</span>}
          </div>
          <span title={report?.source ?? folder?.name}>
            {report ? '已记录清单' : scope} · 来源：{sourceName}
          </span>
          {error && <TaskError error={error} title="移动与隔离未完成" />}
          {reviewing && !running && !preparing && (
            <p className="move-review-note">
              尚未执行本次{action}。
              {prepared
                ? `请勾选底部确认项，再点击“确认${action}并核验”。`
                : '请重新核对清单后再确认。'}
            </p>
          )}
          {!report && kind === 'quarantine' && folder && (
            <p className="move-filter-note">
              本次范围 {selection.length} 张，仅纳入 {eligible.length} 张 0–1 分；未评分与 2–5
              分不参与。
            </p>
          )}
        </div>
        <PreparationStatus task={preparation} preparing={preparing} />
        <BatchOutcome
          report={report}
          reviewing={reviewing || preparing || preparation.cancelled}
          action={action}
        />
        <div className="move-body" ref={body}>
          <div className="move-mode" role="group" aria-label="整理方式">
            {(['quarantine', 'move'] as const).map((value) => (
              <button
                key={value}
                disabled={busy || active}
                aria-pressed={kind === value}
                onClick={() => {
                  setKind(value);
                  setReport(null);
                  setPrepared(false);
                  setAcknowledged(false);
                }}
              >
                {value === 'quarantine'
                  ? '低分隔离 · 0–1'
                  : scope === '已选照片'
                    ? '移动所选照片'
                    : '移动当前范围'}
              </button>
            ))}
          </div>
          <div className="backup-scope">
            <ShieldCheck size={17} />
            <p>
              确认后会改变照片的位置。照片与已有 XMP
              一起移动，保留原文件名、评分与待同步项。仅同卷、新建独立文件夹；不会覆盖或永久删除。完成后可核对并撤回。
            </p>
          </div>
          {!native && (
            <p className="export-preview-note">
              浏览器仅演示清单，不移动文件或修改资料库。请在桌面 App 使用独立照片。
            </p>
          )}
          {active && (
            <p className="backup-error" role="status">
              有未完成的整理，请先在下方继续或撤回，再扫描、评分或导出。
            </p>
          )}
          {report && (
            <div className="move-route">
              <DirectoryPath label="原照片目录" path={report.source} />
              <DirectoryPath
                label={report.direction === 'restore' ? '从这里撤回原位' : '移动到'}
                path={report.destination}
              />
            </div>
          )}
          <div className="export-summary">
            <strong>{statusTitle}</strong>
            <span>{rows.length} 张</span>
          </div>
          <div className="export-file-list" aria-label="整理清单">
            {rows.length ? (
              rows.map((row, i) => (
                <div className={`export-file ${row.error ? 'has-error' : ''}`} key={row.name}>
                  <span className="export-file-index">
                    {!reviewing && !preparing && row.status === 'verified' ? (
                      <Check size={15} />
                    ) : (
                      String(i + 1).padStart(2, '0')
                    )}
                  </span>
                  <div>
                    <strong>{row.name}</strong>
                    <small>
                      {report
                        ? `${row.output}${row.sidecar ? ' · 连同原 XMP' : ' · 无源 XMP'}`
                        : ratingLabel(row.rating)}
                      {row.error ? ` · ${row.error}` : ''}
                    </small>
                  </div>
                  <span>
                    {reviewing || preparing
                      ? `待${action}`
                      : row.status === 'verified'
                        ? '已核验'
                        : row.status === 'error'
                          ? '暂停'
                          : ratingLabel(row.rating)}
                  </span>
                </div>
              ))
            ) : (
              <p className="backup-empty">
                {kind === 'quarantine'
                  ? '当前范围没有 0–1 分照片。未评分不会被隔离。'
                  : '请先打开测试目录并选择照片。'}
              </p>
            )}
          </div>
          {report && !reviewing && !preparing && (
            <div className="export-progress" aria-live="polite">
              <progress max={report.items.length} value={report.verified} />
              <span>
                已{action}并核验 {report.verified} / {report.items.length} 张
              </span>
            </div>
          )}
          {report?.message && <p className="backup-error">{report.message}</p>}
          <details className="task-details">
            <summary>整理限制与恢复说明</summary>
            {report?.safetyBackup && !reviewing && !preparing && (
              <p>已创建资料库安全快照。快照不包含照片，恢复资料库前应先撤回文件整理。</p>
            )}
            <p>
              清单评分为本次操作时记录；撤回保留当前本机评分。
              最多100张；同名照片请分批。停止在当前照片及 XMP
              处理完后生效。中断保留现场，继续或撤回需重新授权原目录和目标父目录。照片/XMP已改变或原位被占用时暂停，不覆盖；空目录与操作记录保留。
            </p>
          </details>
          {!!recent.length && (
            <div className="export-history">
              <h3>最近的安全整理</h3>
              {recent.map((r) => (
                <div className="export-history-row" key={r.id}>
                  <div>
                    <strong>
                      {r.kind === 'quarantine' ? '低分隔离' : '移动'} · {r.items.length} 张
                    </strong>
                    <small>
                      {label(r.status, r.direction)} · {r.destination.split(/[\\/]/).at(-1)}
                    </small>
                  </div>
                  {r.status !== 'restored' && (
                    <div className="move-history-actions">
                      {r.status !== 'completed' && (
                        <button
                          className="button subtle"
                          disabled={busy}
                          onClick={() => void choose(r.id)}
                        >
                          <RotateCcw size={14} />
                          {r.direction === 'restore' ? '继续撤回' : '继续'}
                        </button>
                      )}
                      <button
                        className="button subtle"
                        disabled={busy}
                        onClick={() => void choose(r.id, true)}
                      >
                        核对撤回
                      </button>
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
        {prepared && report && (
          <label className="move-confirm">
            <input
              type="checkbox"
              checked={acknowledged}
              disabled={busy}
              onChange={(e) => setAcknowledged(e.target.checked)}
            />
            我确认{action}这 {rows.length} 张照片，实际文件的位置将改变。
          </label>
        )}
        {report && ['completed', 'restored'].includes(report.status) && !busy && !prepared ? (
          <div className="backup-actions">
            <button className="button primary" onClick={close}>
              完成
            </button>
          </div>
        ) : (
          <div className="backup-actions export-actions move-actions">
            {busy && !running ? (
              <button
                className="button subtle"
                disabled={preparation.cancelling}
                onClick={() => void preparation.cancel().catch((e) => setError(String(e)))}
              >
                {preparation.cancelling ? '正在取消核对…' : '取消核对'}
              </button>
            ) : running ? (
              <button className="button subtle" disabled={stopping} onClick={() => void stop()}>
                {stopping ? '完成当前照片后停止…' : '停止并保留已完成项'}
              </button>
            ) : (
              <button
                className="button subtle"
                disabled={
                  !native || busy || active || !folder || !eligible.length || eligible.length > 100
                }
                onClick={() => void choose()}
              >
                <FolderOpen size={15} />
                {busy ? '正在核对…' : '选择整理位置'}
              </button>
            )}
            <button
              className="button primary"
              disabled={!native || busy || !prepared || !acknowledged}
              onClick={() => void start()}
            >
              {busy ? <LoaderCircle size={15} className="spin" /> : <FolderInput size={15} />}
              {report?.direction === 'restore' ? '确认撤回并核验' : '确认移动并核验'}
            </button>
          </div>
        )}
      </section>
    </div>
  );
}
