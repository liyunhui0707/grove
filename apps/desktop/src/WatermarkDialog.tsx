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
import {
  CalendarDays,
  ChevronLeft,
  ChevronRight,
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
import { companionLabel, type Asset, type Folder } from './domain';
import {
  defaultWatermarkOptions as defaults,
  loadWatermarkStyle,
  saveWatermarkStyle,
  styleFrom,
} from './watermark-style';

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
  onSwitch,
}: {
  folder: Folder;
  assets: Asset[];
  scope: string;
  onClose: () => void;
  onSwitch: () => void;
}) {
  const [selection] = useState(() => assets.map((a) => ({ ...a })));
  const [index, setIndex] = useState(0);
  const [options, setOptions] = useState<WatermarkOptions>(defaults);
  const [styleLoading, setStyleLoading] = useState(true);
  const [styleStatus, setStyleStatus] = useState('正在读取水印样式…');
  const [styleError, setStyleError] = useState('');
  const styleRevision = useRef(0);
  const mounted = useRef(false);
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
  const cityOverride = options.cityOverrides?.[asset?.id];
  const [automaticCity, setAutomaticCity] = useState({ id: '', name: '' });
  const city = cityOverride ?? (automaticCity.id === asset?.id ? automaticCity.name : '');
  const stepPreview = (delta: number) =>
    setIndex((i) => Math.max(0, Math.min(selection.length - 1, i + delta)));
  const remember = (next: WatermarkOptions) => {
    const ticket = ++styleRevision.current;
    setStyleStatus('正在保存样式…');
    setStyleError('');
    void saveWatermarkStyle(styleFrom(next)).then(
      () => {
        if (mounted.current && ticket === styleRevision.current)
          setStyleStatus(native ? '样式已保存' : '演示样式已记住（仅当前页面）');
      },
      (reason) => {
        if (mounted.current && ticket === styleRevision.current) {
          setStyleStatus('');
          setStyleError(`样式未保存：${String(reason)}`);
        }
      },
    );
  };
  const update = (value: Partial<WatermarkOptions>, forceSave = false) => {
    const next = { ...options, ...value };
    setOptions(next);
    if (forceSave || JSON.stringify(styleFrom(next)) !== JSON.stringify(styleFrom(options)))
      remember(next);
    setReport(null);
    setPrepared(false);
    setResuming(false);
  };
  useEffect(() => {
    mounted.current = true;
    let stale = false;
    void loadWatermarkStyle()
      .then(
        (style) => {
          if (stale) return;
          if (style) setOptions({ ...defaults, ...styleFrom({ ...defaults, ...style }) });
          setStyleStatus(
            style ? (native ? '已载入上次样式' : '已载入演示样式') : '正在使用默认样式',
          );
        },
        (reason) => {
          if (!stale) {
            setStyleStatus('');
            setStyleError(`样式读取失败：${String(reason)}。可调整样式后重试保存。`);
          }
        },
      )
      .finally(() => {
        if (!stale) setStyleLoading(false);
      });
    return () => {
      stale = true;
      mounted.current = false;
    };
  }, []);
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
    if (!native || !asset || resuming || styleLoading) return;
    let stale = false;
    setPreview(null);
    setPreviewError('');
    setPreviewBusy(true);
    const timer = window.setTimeout(() => {
      if (composing) return;
      void bridge
        .previewWatermark(asset.id, options)
        .then((result) => {
          if (stale) return;
          setPreview(result);
          if (options.cityOverrides?.[asset.id] === undefined)
            setAutomaticCity({ id: asset.id, name: result.info.source.detectedCity ?? '' });
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
  }, [asset, options, composing, resuming, styleLoading]);
  const refresh = async () =>
    setRecent((await bridge.exportHistory(folder.id)).filter((r) => r.kind === 'watermark'));
  const choose = async (resumeId: string | null = null) => {
    if (busy || styleLoading) return;
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
        setOptions({ ...defaults, ...result.watermark });
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
          if (
            e.key === 'Escape' &&
            !busy &&
            !e.nativeEvent.isComposing &&
            !(e.target instanceof HTMLSelectElement)
          ) {
            e.preventDefault();
            onClose();
          }
          const target = e.target as HTMLElement;
          if (
            !busy &&
            !resuming &&
            !e.nativeEvent.isComposing &&
            !e.altKey &&
            !e.metaKey &&
            !e.ctrlKey &&
            !e.shiftKey &&
            (e.key === 'ArrowLeft' || e.key === 'ArrowRight') &&
            (!target.closest('input, textarea, select, [contenteditable="true"]') ||
              target.matches('.watermark-photo-label select'))
          ) {
            e.preventDefault();
            stepPreview(e.key === 'ArrowLeft' ? -1 : 1);
          }
          if (e.key === 'Tab') {
            const controls = dialogControls(panel.current);
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
            <h2>导出照片</h2>
          </div>
          <button className="icon-button" aria-label="关闭水印" disabled={busy} onClick={onClose}>
            <X size={18} />
          </button>
        </div>
        <ExportTabs watermark disabled={busy} onSwitch={onSwitch} />
        <p className="dialog-intro">
          {resuming ? '继续上次清单 · 参数已固定' : scope} ·{' '}
          {report?.items.length ?? selection.length} 张 · 原件不变
        </p>
        {!resuming && selection.some((asset) => companionLabel(asset)) && (
          <p className="batch-review-note" role="note" aria-label="静态导出范围">
            本次仅生成静态照片的水印 JPEG；关联视频与 RAW 保持原位，不是完整 Live Photo 导出。
          </p>
        )}
        <PreparationStatus task={preparation} preparing={busy && !running} />
        {prepared && <p className="batch-review-note">已准备好。点击下方按钮开始导出。</p>}
        <BatchOutcome
          report={report}
          reviewing={prepared || (busy && !running) || preparation.cancelled}
          action="生成"
        />
        <div className="watermark-body">
          <div className="watermark-workspace">
            <div className="watermark-preview-column">
              <div
                className="watermark-stage"
                role="group"
                tabIndex={resuming ? -1 : 0}
                aria-label="水印预览"
                aria-keyshortcuts="ArrowLeft ArrowRight"
                aria-busy={previewBusy}
              >
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
                    {(options.fallbackDate || city.trim()) && (
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
                        {options.fallbackDate} {city.trim()}
                      </span>
                    )}
                  </div>
                ) : (
                  <p>当前没有照片</p>
                )}
              </div>
              {asset && !resuming && (
                <div className="watermark-photo-label">
                  <button
                    type="button"
                    className="icon-button"
                    aria-label="上一张预览照片"
                    title="上一张（←）"
                    disabled={busy || index === 0}
                    onClick={() => stepPreview(-1)}
                  >
                    <ChevronLeft size={16} />
                  </button>
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
                  <button
                    type="button"
                    className="icon-button"
                    aria-label="下一张预览照片"
                    title="下一张（→）"
                    disabled={busy || index + 1 >= selection.length}
                    onClick={() => stepPreview(1)}
                  >
                    <ChevronRight size={16} />
                  </button>
                </div>
              )}
              {!resuming && (
                <p className="watermark-caption" aria-live="polite">
                  {native
                    ? preview
                      ? `${preview.info.source.dateSource === 'exif' ? 'EXIF 拍摄日期' : preview.info.source.dateSource === 'manual' ? '手动备用日期' : '无拍摄日期'} · ${preview.info.source.text || '不叠加文字'} · ${preview.info.source.width} × ${preview.info.source.height}`
                      : previewError
                    : '浏览器仅布局示意，不读取 EXIF 或生成文件。'}
                </p>
              )}
              {!resuming && preview?.info.source.warning && (
                <p className="watermark-caption">{preview.info.source.warning}</p>
              )}
              {preview && !resuming && (
                <p className="watermark-caption" aria-label="成品尺寸说明">
                  成品 {preview.exportWidth ?? preview.info.source.width} ×{' '}
                  {preview.exportHeight ?? preview.info.source.height} px · sRGB JPEG · 质量{' '}
                  {options.jpegQuality}%
                </p>
              )}
            </div>
            <fieldset
              className="watermark-controls"
              disabled={busy || resuming || styleLoading || !asset}
            >
              <legend className="sr-only">水印样式</legend>
              <p className="watermark-hint">水印外观自动保存。历史任务沿用已确认的清单。</p>
              <div className="watermark-style-memory">
                <span role="status" aria-label="水印样式保存状态">
                  {resuming ? '历史清单参数固定，未改写已记住样式' : styleStatus}
                </span>
                <button
                  className="button subtle"
                  type="button"
                  onClick={() => update(styleFrom(defaults), true)}
                >
                  恢复默认样式
                </button>
              </div>
              {styleError && (
                <div className="backup-error" role="alert">
                  {styleError}
                  <button className="button subtle" type="button" onClick={() => remember(options)}>
                    重试保存样式
                  </button>
                </div>
              )}
              <label>
                城市 <span>{resuming ? '历史清单' : '仅当前照片'}</span>
                <input
                  aria-label="手动城市"
                  placeholder={cityOverride === '' ? '仅显示日期' : '自动识别，也可手填'}
                  maxLength={32}
                  value={resuming ? options.city : city}
                  onCompositionStart={() => setComposing(true)}
                  onCompositionEnd={() => setComposing(false)}
                  onChange={(e) => {
                    update({
                      cityOverrides: { ...options.cityOverrides, [asset.id]: e.target.value },
                    });
                  }}
                />
              </label>
              {!resuming && cityOverride !== undefined && (
                <button
                  type="button"
                  className="button subtle"
                  onClick={() => {
                    const cityOverrides = { ...options.cityOverrides };
                    delete cityOverrides[asset.id];
                    update({ cityOverrides });
                  }}
                >
                  恢复自动识别
                </button>
              )}
              <label>
                备用日期 <span>可留空，仅用于本批缺少拍摄日期的照片</span>
                <input
                  aria-label="备用日期"
                  type="date"
                  value={options.fallbackDate ?? ''}
                  onChange={(e) => update({ fallbackDate: e.target.value || null })}
                />
              </label>
              <p className="watermark-hint">
                逐张用 GPS
                或照片里的城市标签匹配中文名。可以直接修改识别结果，清空后省略城市。日期和城市都为空时，导出的
                JPEG 不叠加文字。
              </p>
              {preview && !preview.canExport && !resuming && (
                <p className="field-error" role="status">
                  {preview.info.source.warning || '当前照片无法导出。'}
                </p>
              )}
              <label className="watermark-checkbox">
                <input
                  type="checkbox"
                  checked={!!options.jpegOnly}
                  onChange={(e) => update({ jpegOnly: e.target.checked })}
                />
                仅导出 JPEG（分享照片）
              </label>
              <label>
                成品尺寸
                <select
                  aria-label="成品尺寸"
                  value={options.outputMaxEdge ?? 'original'}
                  onChange={(e) =>
                    update({
                      outputMaxEdge:
                        e.target.value === 'original'
                          ? null
                          : (Number(e.target.value) as 2048 | 4096),
                    })
                  }
                >
                  <option value="original">原尺寸</option>
                  <option value="2048">分享 · 长边 2048 px</option>
                  <option value="4096">较大 · 长边 4096 px</option>
                </select>
              </label>
              <label>
                JPEG 质量
                <select
                  aria-label="JPEG 质量"
                  value={options.jpegQuality}
                  onChange={(e) => update({ jpegQuality: Number(e.target.value) as 80 | 92 | 98 })}
                >
                  <option value="80">较小文件 · 80%</option>
                  <option value="92">标准 · 92%</option>
                  <option value="98">高画质 · 98%</option>
                </select>
              </label>
              <p className="watermark-hint">
                保持比例，不放大小图；质量值不代表文件大小或无损。每批单独选择。
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
              右下角叠加在原画面内，生成 sRGB JPEG。选择仅导出 JPEG 可省略评分 XMP
              和导出清单。成品不带源 EXIF/GPS；原件、源评分与待同步项不变。支持静态照片；macOS 14+
              可将已支持的 HDR JPEG/HEIC 转为 SDR，不保留 HDR 效果。≤2400万像素、≤32MiB。
            </p>
          </div>
          {error && <TaskError error={error} title="导出照片未完成" />}
          {report && (
            <>
              {report.watermark && (
                <p className="batch-review-note" aria-label="本批导出设置">
                  本批成品：
                  {report.watermark.outputMaxEdge
                    ? `长边至多 ${report.watermark.outputMaxEdge} px`
                    : '原尺寸'}{' '}
                  · JPEG 质量 {report.watermark.jpegQuality ?? 92}% · 不放大 · 参数已固定
                </p>
              )}
              <DirectoryPath label="独立水印文件夹" path={report.destination} />
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
                        {row.caption || '不叠加文字'} → {row.output}
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
              {report.status !== 'completed' && !prepared && !(busy && !running) && (
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
          <details className="task-details">
            <summary>导出限制与中断恢复</summary>
            <p>
              取消会保留部分成果。可在下方继续，重新选择原目标父目录；不会覆盖同名未知文件。预览为缩小版本，成品按本批尺寸与质量生成。原格式副本仍从“导出副本”获取。
            </p>
          </details>
          {!!recent.length && (
            <div className="export-history">
              <h3>最近的水印导出</h3>
              {recent.map((r) => (
                <div className="export-history-row" key={r.id}>
                  <div>
                    <strong>{exportHistoryLabel(r.destination, r.kind, r.items.length)}</strong>
                    <small>
                      {status(r.status)} · {r.verified}/{r.items.length} 张
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
                      disabled={busy || styleLoading}
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
                {cancelling ? '正在停止…' : '停止并保留成果'}
              </button>
            ) : (
              <button
                className="button subtle"
                disabled={
                  !native ||
                  busy ||
                  !selection.length ||
                  resuming ||
                  !preview?.canExport ||
                  previewBusy ||
                  composing ||
                  styleLoading
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
              生成水印照片
            </button>
          </div>
        )}
      </section>
    </div>
  );
}
