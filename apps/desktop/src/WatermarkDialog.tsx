import { usePreparation, PreparationStatus, BatchOutcome } from './BatchFeedback';
import { useEffect, useRef, useState } from 'react';
import {
  CalendarDays,
  Check,
  FolderOpen,
  LoaderCircle,
  RotateCcw,
  ShieldCheck,
  X,
} from 'lucide-react';
import {
  bridge,
  mediaSource,
  native,
  type ExportReport,
  type WatermarkOptions,
  type WatermarkPreview,
} from './bridge';
import type { Asset, Folder } from './domain';

const defaults: WatermarkOptions = {
  city: '',
  fallbackDate: null,
  font: 'sans',
  sizePercent: 2,
  color: '#ffffff',
  opacity: 0.88,
  marginPercent: 2.5,
  shadow: true,
};
const status = (s: string) =>
  ({
    ready: '清单已核对，等待确认',
    running: '正在生成与核验',
    completed: '水印已核验',
    cancelled: '已停止，保留现有成果',
    partial: '部分完成，请检查结果',
  })[s] ?? s;

export function WatermarkDialog({
  folder,
  assets,
  scope,
  onClose,
}: {
  folder: Folder;
  assets: Asset[];
  scope: string;
  onClose: () => void;
}) {
  const [selection] = useState(() => assets.map((a) => ({ ...a })));
  const [index, setIndex] = useState(0);
  const [options, setOptions] = useState<WatermarkOptions>(defaults);
  const [preview, setPreview] = useState<WatermarkPreview | null>(null);
  const [previewError, setPreviewError] = useState('');
  const [previewBusy, setPreviewBusy] = useState(false);
  const [composing, setComposing] = useState(false);
  const [report, setReport] = useState<ExportReport | null>(null);
  const [recent, setRecent] = useState<ExportReport[]>([]);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [running, setRunning] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [prepared, setPrepared] = useState(false);
  const [resuming, setResuming] = useState(false);
  const panel = useRef<HTMLElement>(null);
  const preparation = usePreparation(bridge.cancelExport);
  const asset = selection[index];
  const update = (value: Partial<WatermarkOptions>) => {
    setOptions((o) => ({ ...o, ...value }));
    setReport(null);
    setPrepared(false);
    setResuming(false);
  };
  useEffect(() => {
    panel.current?.querySelector<HTMLButtonElement>('button')?.focus();
    let stale = false;
    if (native)
      void bridge
        .exportHistory(folder.id)
        .then((items) => {
          if (!stale) setRecent(items.filter((r) => r.kind === 'watermark'));
        })
        .catch((e) => {
          if (!stale) setError(String(e));
        });
    return () => {
      stale = true;
    };
  }, [folder.id]);
  useEffect(() => {
    if (!native || !asset || resuming) return;
    let stale = false;
    setPreview(null);
    setPreviewError('');
    setPreviewBusy(true);
    const timer = window.setTimeout(() => {
      if (composing) return;
      void bridge
        .previewWatermark(asset.id, options)
        .then((result) => {
          if (!stale) setPreview(result);
        })
        .catch((e) => {
          if (!stale) setPreviewError(String(e));
        })
        .finally(() => {
          if (!stale) setPreviewBusy(false);
        });
    }, 280);
    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [asset, options, composing, resuming]);
  const refresh = async () =>
    setRecent((await bridge.exportHistory(folder.id)).filter((r) => r.kind === 'watermark'));
  const choose = async (resumeId: string | null = null) => {
    if (busy) return;
    const ticket = preparation.begin();
    setBusy(true);
    setError('');
    setPrepared(false);
    try {
      const result = await bridge.chooseWatermark(
        folder.id,
        resumeId ? [] : selection,
        options,
        resumeId,
        preparation.onProgress(ticket),
      );
      if (result && !preparation.ignored(ticket)) {
        if (result.kind !== 'watermark' || !result.watermark)
          throw Error('导出记录类型不符，请重新打开面板');
        setOptions(result.watermark);
        setReport(result);
        setPrepared(true);
        setResuming(resumeId !== null);
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
    if (!prepared || !report || busy) return;
    setBusy(true);
    setRunning(true);
    setPrepared(false);
    setError('');
    try {
      setReport(await bridge.confirmExport(report.id, setReport));
      await refresh();
    } catch (e) {
      setError(String(e));
      try {
        await refresh();
      } catch {
        /* Keep original failure visible. */
      }
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
  return (
    <div
      className="modal-backdrop"
      onClick={() => {
        if (!busy) onClose();
      }}
    >
      <section
        className="backup-dialog watermark-dialog"
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-label="日期城市水印"
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === 'Escape' && !busy && !e.nativeEvent.isComposing) {
            e.preventDefault();
            onClose();
          }
          if (e.key === 'Tab') {
            const controls = Array.from(
              panel.current?.querySelectorAll<HTMLElement>(
                'button:not(:disabled),input:not(:disabled),select:not(:disabled),summary',
              ) ?? [],
            ).filter(
              (el) =>
                el.getClientRects().length > 0 &&
                !(el.tagName !== 'SUMMARY' && el.closest('details:not([open])')),
            );
            const active = document.activeElement as HTMLElement;
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
          <CalendarDays size={22} />
          <div>
            <span className="eyebrow">A PLACE AND A MOMENT</span>
            <h2>让那一天，留在画面里。</h2>
          </div>
          <button className="icon-button" aria-label="关闭水印" disabled={busy} onClick={onClose}>
            <X size={18} />
          </button>
        </div>
        <p className="dialog-intro">
          {resuming ? '继续上次清单 · 参数已固定' : scope} ·{' '}
          {report?.items.length ?? selection.length} 张 · 原件不变
        </p>
        <PreparationStatus task={preparation} preparing={busy && !running} />
        {prepared && (
          <p className="batch-review-note">清单已核对，尚未开始本次文件操作。确认后才会执行。</p>
        )}
        <BatchOutcome
          report={report}
          reviewing={prepared || (busy && !running) || preparation.cancelled}
          action="生成"
        />
        <div className="watermark-body">
          <div className="watermark-workspace">
            <div className="watermark-preview-column">
              <div className="watermark-stage" aria-label="水印预览" aria-busy={previewBusy}>
                {resuming ? (
                  <p>
                    使用上次保存的日期和样式。
                    <br />
                    请核对下方逐张清单后继续。
                  </p>
                ) : native ? (
                  preview ? (
                    <img src={preview.dataUrl} alt="原生水印预览" />
                  ) : (
                    <p>{previewError || (previewBusy ? '正在生成预览…' : '请选择照片')}</p>
                  )
                ) : asset ? (
                  <div className="watermark-illustration">
                    <img src={mediaSource(asset, 1600)} alt="水印布局示意" />
                    {options.fallbackDate && (
                      <span
                        style={{
                          right: `${options.marginPercent}%`,
                          bottom: `${options.marginPercent}%`,
                          fontSize: `${options.sizePercent}cqw`,
                          color: options.color,
                          opacity: options.opacity,
                          fontFamily: options.font === 'serif' ? 'serif' : 'sans-serif',
                          textShadow: options.shadow ? '0 1px 3px #0008' : 'none',
                        }}
                      >
                        {options.fallbackDate} {options.city.trim()}
                      </span>
                    )}
                  </div>
                ) : (
                  <p>当前没有照片</p>
                )}
              </div>
              {asset && !resuming && (
                <label className="watermark-photo-label">
                  预览照片
                  <select
                    aria-label="预览照片"
                    value={index}
                    onChange={(e) => setIndex(Number(e.target.value))}
                    disabled={busy}
                  >
                    {selection.map((a, i) => (
                      <option value={i} key={a.id}>
                        {i + 1}. {a.relative}
                      </option>
                    ))}
                  </select>
                </label>
              )}
              <p className="watermark-caption" aria-live="polite">
                {native
                  ? preview
                    ? `${preview.info.source.dateSource === 'exif' ? 'EXIF 拍摄日期' : preview.info.source.dateSource === 'manual' ? '手动备用日期' : '缺少拍摄日期，请填写备用日期'} · ${preview.info.source.text || '尚无水印'} · ${preview.info.source.width} × ${preview.info.source.height}`
                    : previewError
                  : '浏览器仅布局示意，不读取 EXIF 或生成文件。'}
              </p>
              {preview?.info.source.warning && (
                <p className="watermark-caption">{preview.info.source.warning}</p>
              )}
            </div>
            <fieldset className="watermark-controls" disabled={busy || resuming}>
              <legend className="sr-only">水印样式</legend>
              <label>
                城市 <span>可选</span>
                <input
                  aria-label="手动城市"
                  placeholder="例如：温州"
                  maxLength={32}
                  value={options.city}
                  onCompositionStart={() => setComposing(true)}
                  onCompositionEnd={() => setComposing(false)}
                  onChange={(e) => update({ city: e.target.value })}
                />
              </label>
              <label>
                备用日期 <span>仅用于缺少 EXIF 时</span>
                <input
                  aria-label="备用日期"
                  type="date"
                  value={options.fallbackDate ?? ''}
                  onChange={(e) => update({ fallbackDate: e.target.value || null })}
                />
              </label>
              <p className="watermark-hint">
                优先使用实际拍摄日期。城市留空时仅显示日期；不联网查询位置。
              </p>
              <label>
                字体
                <select
                  aria-label="水印字体"
                  value={options.font}
                  onChange={(e) => update({ font: e.target.value as WatermarkOptions['font'] })}
                >
                  <option value="sans">简洁无衬线</option>
                  <option value="serif">经典衬线</option>
                </select>
              </label>
              <label>
                字号 <output>{options.sizePercent.toFixed(1)}%</output>
                <input
                  aria-label="水印字号"
                  type="range"
                  min="0.8"
                  max="5"
                  step="0.1"
                  value={options.sizePercent}
                  onChange={(e) => update({ sizePercent: Number(e.target.value) })}
                />
              </label>
              <details>
                <summary>更多样式</summary>
                <div className="watermark-advanced">
                  <label>
                    文字颜色
                    <input
                      aria-label="水印颜色"
                      type="color"
                      value={options.color}
                      onChange={(e) => update({ color: e.target.value })}
                    />
                  </label>
                  <label>
                    透明度 <output>{Math.round(options.opacity * 100)}%</output>
                    <input
                      aria-label="水印透明度"
                      type="range"
                      min="0.15"
                      max="1"
                      step="0.01"
                      value={options.opacity}
                      onChange={(e) => update({ opacity: Number(e.target.value) })}
                    />
                  </label>
                  <label>
                    边距 <output>{options.marginPercent.toFixed(1)}%</output>
                    <input
                      aria-label="水印边距"
                      type="range"
                      min="0.5"
                      max="8"
                      step="0.1"
                      value={options.marginPercent}
                      onChange={(e) => update({ marginPercent: Number(e.target.value) })}
                    />
                  </label>
                  <label className="watermark-checkbox">
                    <input
                      type="checkbox"
                      checked={options.shadow}
                      onChange={(e) => update({ shadow: e.target.checked })}
                    />
                    轻柔阴影
                  </label>
                </div>
              </details>
            </fieldset>
          </div>
          <div className="backup-scope">
            <ShieldCheck size={17} />
            <p>
              右下角叠加在原画面内，独立导出 sRGB JPEG 与当前评分 XMP。成品不带源
              EXIF/GPS；原件、源评分与待同步项不变。支持静态照片；macOS 14+ 可将已支持的 HDR
              JPEG/HEIC 转为 SDR，不保留 HDR 效果。≤2400万像素、≤32MiB。
            </p>
          </div>
          {error && (
            <p className="backup-error" role="alert">
              {error}
            </p>
          )}
          {selection.length > 100 && !resuming && (
            <p className="backup-error">单次最多100张，请缩小选择。</p>
          )}
          {report && (
            <>
              <div className="export-destination">
                <span>独立水印文件夹</span>
                <strong>{report.destination}</strong>
              </div>
              <div className="export-summary">
                <strong>
                  {prepared
                    ? '清单已核对，等待确认'
                    : busy && !running
                      ? '正在核对清单'
                      : status(report.status)}
                </strong>
                <span>
                  {prepared || (busy && !running)
                    ? `${report.items.length} 张`
                    : `${report.verified}/${report.items.length} 张`}
                </span>
              </div>
              <div className="export-file-list" aria-label="水印导出清单">
                {report.items.map((row, i) => (
                  <div className={`export-file ${row.error ? 'has-error' : ''}`} key={row.output}>
                    <span className="export-file-index">
                      {!prepared && !(busy && !running) && row.status === 'verified' ? (
                        <Check size={15} />
                      ) : (
                        i + 1
                      )}
                    </span>
                    <div>
                      <strong>{row.name}</strong>
                      <small>
                        {row.caption} → {row.output}
                        {row.error && ` · ${row.error}`}
                      </small>
                    </div>
                    <span>
                      {prepared || (busy && !running)
                        ? '待生成'
                        : row.status === 'verified'
                          ? '已核验'
                          : row.status === 'error'
                            ? '暂停'
                            : '待生成'}
                    </span>
                  </div>
                ))}
              </div>
              {!prepared && !(busy && !running) && (
                <div className="export-progress" aria-live="polite">
                  <progress max={report.items.length} value={report.verified} />
                  <span>
                    已核验 {report.verified} / {report.items.length} 张
                  </span>
                </div>
              )}
              {report.message && <p className="backup-error">{report.message}</p>}
            </>
          )}
          <p className="sync-footnote">
            取消会保留部分成果。可在下方继续，重新选择原目标父目录；不会覆盖同名未知文件。预览为缩小版本，成品保持方向归正后的原尺寸。
          </p>
          {!!recent.length && (
            <div className="export-history">
              <h3>最近的水印导出</h3>
              {recent.map((r) => (
                <div className="export-history-row" key={r.id}>
                  <div>
                    <strong>{r.destination.split(/[\\/]/).at(-1)}</strong>
                    <small>
                      {status(r.status)} · {r.verified}/{r.items.length} 张
                    </small>
                  </div>
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
              {cancelling ? '正在停止…' : '停止并保留成果'}
            </button>
          ) : (
            <button
              className="button subtle"
              disabled={
                !native ||
                busy ||
                !selection.length ||
                selection.length > 100 ||
                resuming ||
                !preview?.canExport ||
                previewBusy ||
                composing
              }
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
            {busy ? <LoaderCircle size={15} className="spin" /> : <CalendarDays size={15} />}
            确认生成并核验
          </button>
        </div>
      </section>
    </div>
  );
}
