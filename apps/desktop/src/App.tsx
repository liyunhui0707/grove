import { GuideDialog } from './GuideDialog';
import { ApplicationBadge } from './ApplicationBadge';
import { ExitSaveGuard } from './ExitSaveGuard';
import { restoreReview, type ReviewView } from './workspace';
import { createWorkspaceWriter } from './workspace-writer';
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type Ref,
} from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import {
  ArrowDownWideNarrow,
  ArrowLeft,
  ArrowRight,
  Archive,
  CalendarDays,
  Check,
  ChevronLeft,
  ChevronRight,
  Circle,
  CircleSlash,
  Columns2,
  Copy,
  FolderOpen,
  FolderInput,
  FileCheck2,
  Grid2X2,
  ImageOff,
  Keyboard,
  BookOpen,
  Leaf,
  Link2,
  LoaderCircle,
  Maximize2,
  Minus,
  Moon,
  Plus,
  RotateCcw,
  Search,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  Star,
  Sun,
  Undo2,
  X,
} from 'lucide-react';
import { bridge, native } from './bridge';
import {
  applyChanges,
  companionLabel,
  filterAssets,
  gridColumns,
  isTyping,
  nextId,
  nextComparisonId,
  ratingLabel,
  type Asset,
  type Filter,
  type Folder,
  type Rating,
  type Sort,
  type SkippedMedia,
} from './domain';
import { useMedia } from './media';
import { BackupDialog } from './BackupDialog';
import { SyncDialog } from './SyncDialog';
import { RelocationDialog } from './RelocationDialog';
import { ExportDialog } from './ExportDialog';
import { WatermarkDialog } from './WatermarkDialog';
import { MoveDialog } from './MoveDialog';

type Mode = 'grid' | 'review' | 'compare';
type Viewport = { scale: number; x: number; y: number };
const fit: Viewport = { scale: 1, x: 0, y: 0 };
type History = {
  token: string;
  targets: string[];
  active: string;
  queue: string[];
  compare: string[];
  mode: Mode;
  selection: string[];
  filter: Filter;
  search: string;
  sort: Sort;
};
function IconButton({
  children,
  label,
  onClick,
  active,
  disabled = false,
  buttonRef,
}: {
  children: ReactNode;
  label: string;
  onClick: () => void;
  active?: boolean;
  disabled?: boolean;
  buttonRef?: Ref<HTMLButtonElement>;
}) {
  return (
    <button
      type="button"
      ref={buttonRef}
      className={`icon-button ${active ? 'is-active' : ''}`}
      title={label}
      aria-label={label}
      onClick={onClick}
      disabled={disabled}
    >
      {children}
    </button>
  );
}
function RatingButtons({
  value,
  onRate,
  disabled,
}: {
  value: Rating;
  onRate: (r: Rating) => void;
  disabled: boolean;
}) {
  return (
    <div className="rating-buttons" aria-label="设置评分">
      <button
        disabled={disabled}
        className={`reject-score ${value === 0 ? 'rated' : ''}`}
        title="0 · 淘汰候选"
        aria-label="0 分：淘汰候选"
        onClick={() => onRate(0)}
      >
        <CircleSlash size={17} />
        <span>0</span>
      </button>
      <span className="rating-separator" />
      {([1, 2, 3, 4, 5] as const).map((n) => (
        <button
          key={n}
          disabled={disabled}
          className={value !== null && value >= n ? 'star-score rated' : 'star-score'}
          aria-label={`${n} 星`}
          title={`${n} · ${n} 星`}
          onClick={() => onRate(n)}
        >
          <Star size={18} />
          <span>{n}</span>
        </button>
      ))}
      <button
        disabled={disabled}
        className="clear-score"
        title="U · 清除评分"
        aria-label="清除评分"
        onClick={() => onRate(null)}
      >
        <Minus size={16} />
      </button>
    </div>
  );
}
function RatingBadge({ value }: { value: Rating }) {
  return (
    <span
      className={`rating-badge ${value === 0 ? 'rejected' : value === null ? 'unreviewed' : 'has-stars'}`}
    >
      {value === null ? (
        <Circle size={10} />
      ) : value === 0 ? (
        <CircleSlash size={11} />
      ) : (
        <Star size={11} fill="currentColor" />
      )}
      {value === null ? '未评分' : value === 0 ? '淘汰候选' : value}
    </span>
  );
}
function Thumbnail({ asset }: { asset: Asset }) {
  const media = useMedia(asset, 480);
  return media.error ? (
    <span className="thumb-error" title={media.error}>
      <ImageOff size={24} />
      <span>无法预览</span>
    </span>
  ) : media.url ? (
    <img src={media.url} alt={asset.name} draggable={false} />
  ) : (
    <span className="skeleton" />
  );
}
function Gallery({
  assets,
  selected,
  active,
  size,
  initialOffset,
  restorePosition,
  onOffsetRestored,
  revealId,
  onRevealed,
  onOffset,
  onSelect,
  onOpen,
}: {
  assets: Asset[];
  selected: string[];
  active: string;
  size: number;
  initialOffset: number;
  restorePosition: { offset: number; count: number } | null;
  onOffsetRestored: () => void;
  revealId: string | null;
  onRevealed: () => void;
  onOffset: (n: number) => void;
  onSelect: (id: string, extend: boolean, range: boolean) => void;
  onOpen: (id: string) => void;
}) {
  const parent = useRef<HTMLDivElement>(null);
  const [measuredWidth, setWidth] = useState(0);
  const width = measuredWidth || 1000;
  const pendingReveal = useRef(revealId);
  useEffect(() => {
    const el = parent.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const columns = gridColumns(width, size);
  const rowHeight = ((width - (columns - 1) * 20) / columns) * 0.73 + 53;
  const virtual = useVirtualizer({
    count: Math.ceil(assets.length / columns),
    getScrollElement: () => parent.current,
    estimateSize: () => rowHeight,
    overscan: 2,
    initialOffset,
  });
  const totalSize = virtual.getTotalSize();
  useLayoutEffect(() => {
    virtual.measure();
  }, [rowHeight, virtual]);
  useLayoutEffect(() => {
    // Native scans stream batches: the grid can mount before the saved view
    // arrives. Apply it once after scan completion and real column measurement;
    // initialOffset alone is only read when the virtualizer first mounts.
    if (restorePosition === null || !measuredWidth || assets.length !== restorePosition.count)
      return;
    // Density changes invalidate cached estimates. Wait for the render using
    // the new row sizes, so a long saved offset clamps against the final list.
    if (Math.abs(totalSize - Math.ceil(assets.length / columns) * rowHeight) > 1) return;
    const element = parent.current;
    const spacer = element?.firstElementChild;
    if (!element || !spacer) return;
    // The virtualizer can report the final total before its spacer has that
    // layout size. A command then clamps to the previous scan batch's bottom.
    // Observe the actual spacer rather than retiring that restore too early.
    let frame = 0;
    const schedule = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        if (
          !element.clientHeight ||
          Math.abs(spacer.getBoundingClientRect().height - totalSize) > 1
        )
          return;
        virtual.scrollToOffset(restorePosition.offset);
        onOffsetRestored();
      });
    };
    const observer = new ResizeObserver(schedule);
    observer.observe(spacer);
    observer.observe(element);
    schedule();
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  }, [
    restorePosition,
    measuredWidth,
    assets.length,
    columns,
    rowHeight,
    totalSize,
    virtual,
    onOffsetRestored,
  ]);
  useLayoutEffect(() => {
    // Returning from review is a one-shot request. Wait for the real column
    // geometry, then let ordinary scrolling/selection proceed undisturbed.
    const id = pendingReveal.current;
    if (!id || !measuredWidth) return;
    const index = assets.findIndex((asset) => asset.id === id);
    if (index >= 0)
      virtual.scrollToIndex(Math.floor(index / columns), {
        // An oversized row aligned to its bottom can clip the photo's top.
        align: rowHeight > (parent.current?.clientHeight ?? 0) ? 'start' : 'auto',
      });
    pendingReveal.current = null;
    onRevealed();
  }, [assets, columns, measuredWidth, onRevealed, rowHeight, virtual]);
  return (
    <div
      className="gallery-viewport"
      ref={parent}
      onScroll={(e) => onOffset(e.currentTarget.scrollTop)}
      aria-label="照片网格"
    >
      <div style={{ height: totalSize, position: 'relative' }}>
        {virtual.getVirtualItems().map((row) => (
          <div
            className="photo-row"
            key={row.key}
            style={{
              transform: `translateY(${row.start}px)`,
              gridTemplateColumns: `repeat(${columns},minmax(0,1fr))`,
            }}
          >
            {assets.slice(row.index * columns, (row.index + 1) * columns).map((asset) => (
              <article
                key={asset.id}
                className={`photo-card ${selected.includes(asset.id) ? 'selected' : ''} ${active === asset.id ? 'focused' : ''}`}
              >
                <button
                  className="photo-open"
                  aria-label={`查看 ${asset.name}`}
                  onClick={(e) => onSelect(asset.id, e.metaKey || e.ctrlKey, e.shiftKey)}
                  onDoubleClick={() => onOpen(asset.id)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      e.preventDefault();
                      onOpen(asset.id);
                    }
                  }}
                >
                  <Thumbnail asset={asset} />
                  {asset.format === 'heic' || asset.format === 'heif' ? (
                    <span className="format-tag">HEIC</span>
                  ) : null}
                </button>
                <button
                  className="select-photo"
                  aria-label={`选择 ${asset.name}`}
                  aria-pressed={selected.includes(asset.id)}
                  onClick={(e) => {
                    e.stopPropagation();
                    onSelect(asset.id, true, false);
                  }}
                >
                  {selected.includes(asset.id) && <Check size={13} />}
                </button>
                <div className="photo-caption">
                  <span title={asset.relative}>{asset.name}</span>
                  <RatingBadge value={asset.rating} />
                </div>
              </article>
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}
function PhotoPane({
  asset,
  active,
  viewport,
  onViewport,
  onActivate,
  onFocus,
  detail,
  onRate,
  disabled,
}: {
  asset: Asset;
  active: boolean;
  viewport: Viewport;
  onViewport: (v: Viewport) => void;
  onActivate: () => void;
  onFocus: () => void;
  detail: boolean;
  onRate: (r: Rating) => void;
  disabled: boolean;
}) {
  const media = useMedia(asset, detail ? 8192 : 1600);
  const canvas = useRef<HTMLDivElement>(null);
  const viewportRef = useRef(viewport);
  viewportRef.current = viewport;
  const [imageError, setImageError] = useState(false);
  useEffect(() => setImageError(false), [media.url]);
  useEffect(() => {
    const el = canvas.current;
    if (!el) return;
    const wheel = (e: WheelEvent) => {
      e.preventDefault();
      onActivate();
      const v = viewportRef.current;
      onViewport({ ...v, scale: Math.max(1, Math.min(6, v.scale * Math.exp(-e.deltaY * 0.002))) });
    };
    el.addEventListener('wheel', wheel, { passive: false });
    return () => el.removeEventListener('wheel', wheel);
  }, [onViewport, onActivate]);
  const drag = useRef<{ x: number; y: number; view: Viewport } | null>(null);
  return (
    <section
      className={`photo-pane ${active ? 'active-pane' : ''}`}
      aria-label={`比较照片 ${asset.name}`}
      onPointerDown={onActivate}
    >
      <div
        className="pane-canvas"
        ref={canvas}
        onDoubleClick={onFocus}
        onPointerDown={(e) => {
          if (e.button !== 0 || viewport.scale <= 1) return;
          drag.current = { x: e.clientX, y: e.clientY, view: viewport };
          e.currentTarget.setPointerCapture(e.pointerId);
        }}
        onPointerMove={(e) => {
          const start = drag.current;
          if (!start) return;
          const rect = e.currentTarget.getBoundingClientRect();
          onViewport({
            ...start.view,
            x: Math.max(-2, Math.min(2, start.view.x + (e.clientX - start.x) / rect.width)),
            y: Math.max(-2, Math.min(2, start.view.y + (e.clientY - start.y) / rect.height)),
          });
        }}
        onPointerUp={() => {
          drag.current = null;
        }}
        onPointerCancel={() => {
          drag.current = null;
        }}
      >
        {media.error || imageError ? (
          <div className="preview-error">
            <ImageOff size={30} />
            <strong>这张照片暂时无法预览</strong>
            <span>{media.error || '图像显示失败，可重试或继续查看其他照片'}</span>
            <button
              type="button"
              className="secondary"
              onPointerDown={(e) => e.stopPropagation()}
              onDoubleClick={(e) => e.stopPropagation()}
              onClick={(e) => {
                e.stopPropagation();
                setImageError(false);
                media.retry();
              }}
            >
              重试预览
            </button>
          </div>
        ) : media.url ? (
          <img
            src={media.url}
            alt={asset.name}
            draggable={false}
            onError={() => setImageError(true)}
            style={{
              transform: `translate(${viewport.x * 100}%,${viewport.y * 100}%) scale(${viewport.scale})`,
            }}
          />
        ) : (
          <div className="preview-loading">
            <LoaderCircle className="spin" size={22} />
            <span>正在准备清晰预览</span>
          </div>
        )}
      </div>
      <div className="pane-footer">
        <span className="pane-name">
          {asset.name}
          <small>
            {asset.width && asset.height
              ? `${asset.width} × ${asset.height}`
              : asset.format.toUpperCase()}
            {detail ? ' · 细节预览（最长边 8192px）' : ''}
            {companionLabel(asset) ? ` · ${companionLabel(asset)}` : ''}
          </small>
        </span>
        <RatingButtons value={asset.rating} onRate={onRate} disabled={disabled} />
      </div>
    </section>
  );
}

export function App() {
  const [theme, setTheme] = useState(
    () =>
      localStorage.getItem('moment-grove-theme') ??
      (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'),
  );
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem('moment-grove-theme', theme);
  }, [theme]);
  const [folder, setFolder] = useState<Folder | null>(null);
  const [assets, setAssets] = useState<Asset[]>([]);
  const [loading, setLoading] = useState(native);
  const [workspaceReady, setWorkspaceReady] = useState(!native);
  const [workspaceStatus, setWorkspaceStatus] = useState('');
  const [workspaceError, setWorkspaceError] = useState('');
  const [gridOffset, setGridOffset] = useState(0);
  const [gridRestorePosition, setGridRestorePosition] = useState<{
    offset: number;
    count: number;
  } | null>(null);
  const clearGridRestore = useCallback(() => setGridRestorePosition(null), []);
  const workspaceWrites = useRef(
    createWorkspaceWriter(({ folderId, view }) => bridge.saveWorkspace(folderId, view)),
  );
  const workspaceRevision = useRef(0);
  const reviewWrite = useRef<{ done: Promise<void>; finish: () => void } | null>(null);
  const reviewWriteError = useRef('');
  const reviewErrorFolder = useRef<number | null>(null);
  const reviewErrorIds = useRef<string[]>([]);
  const [reviewFailure, setReviewFailure] = useState('');
  const [scanning, setScanning] = useState(false);
  const [scanOutcome, setScanOutcome] = useState<'complete' | 'cancelled' | 'failed' | null>(null);
  const [saving, setSaving] = useState(false);
  const [reviewInputHint, setReviewInputHint] = useState('');
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [warnings, setWarnings] = useState<string[]>([]);
  const [scanSummary, setScanSummary] = useState<{ count: number; skipped: SkippedMedia } | null>(
    null,
  );
  const [filter, setFilter] = useState<Filter>('all');
  const [sort, setSort] = useState<Sort>('name');
  const [search, setSearch] = useState('');
  const [size, setSize] = useState(260);
  const [selected, setSelected] = useState<string[]>([]);
  const [active, setActive] = useState('');
  const [mode, setMode] = useState<Mode>('grid');
  const [reviewQueue, setReviewQueue] = useState<string[]>([]);
  const [compareIds, setCompareIds] = useState<string[]>([]);
  const [detail, setDetail] = useState(false);
  const [linked, setLinked] = useState(true);
  const [linkedViewport, setLinkedViewport] = useState<Viewport>(fit);
  const [viewports, setViewports] = useState<Record<string, Viewport>>({});
  const [autoAdvance, setAutoAdvance] = useState(true);
  const [history, setHistory] = useState<History[]>([]);
  const [help, setHelp] = useState(false);
  const [guideOpen, setGuideOpen] = useState(false);
  const guideReturnFocus = useRef<HTMLButtonElement>(null);
  const guideOrigin = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!guideOpen && guideOrigin.current?.isConnected) guideOrigin.current.focus();
  }, [guideOpen]);
  const [backupsOpen, setBackupsOpen] = useState(false);
  const [syncOpen, setSyncOpen] = useState(false);
  const [relocationOpen, setRelocationOpen] = useState(false);
  const [moveOpen, setMoveOpen] = useState(false);
  const moveReturnFocus = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!moveOpen && moveReturnFocus.current?.isConnected) moveReturnFocus.current.focus();
  }, [moveOpen]);
  const [watermarkOpen, setWatermarkOpen] = useState(false);
  const watermarkReturnFocus = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!watermarkOpen && watermarkReturnFocus.current?.isConnected)
      watermarkReturnFocus.current.focus();
  }, [watermarkOpen]);
  const [exportOpen, setExportOpen] = useState(false);
  const exportReturnFocus = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!exportOpen && exportReturnFocus.current?.isConnected) exportReturnFocus.current.focus();
  }, [exportOpen]);
  const relocationReturnFocus = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!relocationOpen && relocationReturnFocus.current?.isConnected)
      relocationReturnFocus.current.focus();
  }, [relocationOpen]);
  const syncReturnFocus = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!syncOpen && syncReturnFocus.current?.isConnected) syncReturnFocus.current.focus();
  }, [syncOpen]);
  const backupReturnFocus = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!backupsOpen && backupReturnFocus.current?.isConnected) backupReturnFocus.current.focus();
  }, [backupsOpen]);
  useEffect(() => {
    if (!help) return;
    const previous = document.activeElement;
    document.querySelector<HTMLButtonElement>('.shortcut-dialog button')?.focus();
    return () => {
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, [help]);
  const scanGeneration = useRef(0);
  const scrollOffset = useRef(0);
  const gridReturnTarget = useRef<string | null>(null);
  const saveLock = useRef(false);
  const visible = useMemo(
    () => filterAssets(assets, filter, search, sort),
    [assets, filter, search, sort],
  );
  const byId = useMemo(() => new Map(assets.map((a) => [a.id, a])), [assets]);
  const unreviewed = assets.filter((a) => a.rating === null).length;
  const picks = assets.filter((a) => (a.rating ?? -1) >= 4).length;
  const snapshot = useRef({ active, mode, folder, reviewQueue, autoAdvance });
  snapshot.current = { active, mode, folder, reviewQueue, autoAdvance };
  const toast = useCallback((message: string) => setNotice(message), []);
  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(''), 4000);
    return () => clearTimeout(timer);
  }, [notice]);

  const open = async (demo: boolean, provided?: Folder, restored?: ReviewView) => {
    if (saveLock.current || (loading && !restored)) return;
    if (reviewWriteError.current && provided?.id !== reviewErrorFolder.current) {
      toast('请先重新扫描当前文件夹，核对这次评分或撤销的保存结果。');
      return;
    }
    setError('');
    setLoading(true);
    let scanStarted = false;
    try {
      const chosen = provided ?? (demo ? await bridge.demo() : await bridge.chooseFolder());
      if (!chosen) return;
      await bridge.cancel();
      const generation = ++scanGeneration.current;
      setFolder(chosen);
      setAssets([]);
      setSelected([]);
      setActive('');
      setHistory([]);
      setReviewInputHint('');
      setMode('grid');
      setDetail(false);
      setFilter('all');
      setSearch('');
      setWarnings([]);
      setScanSummary(null);
      scrollOffset.current = 0;
      setGridOffset(0);
      setGridRestorePosition(null);
      setWorkspaceStatus('');
      const scanned: Asset[] = [];
      let scanCompleted = false;
      setScanning(true);
      setScanOutcome(null);
      scanStarted = true;
      await bridge.scan(chosen.id, (event) => {
        if (scanGeneration.current !== generation) return;
        if (event.kind === 'batch') {
          scanned.push(...event.items);
          setAssets((current) => [...current, ...event.items]);
        }
        if (event.kind === 'done') {
          scanCompleted = true;
          setScanning(false);
          setWarnings(event.warnings);
          setScanSummary({
            count: event.count,
            skipped: event.skipped ?? { videos: 0, raw: 0, images: 0 },
          });
          setScanOutcome('complete');
          if (restored) {
            const resume = restoreReview(scanned, restored);
            setFilter(restored.filter);
            setSearch(restored.search);
            setSort(restored.sort);
            setActive(resume.active);
            setReviewQueue(resume.queue);
            setMode(resume.mode);
            const count = filterAssets(
              scanned,
              restored.filter,
              restored.search,
              restored.sort,
            ).length;
            const offset = count ? restored.offset : 0;
            scrollOffset.current = offset;
            setGridOffset(offset);
            setGridRestorePosition(resume.mode === 'grid' && count ? { offset, count } : null);
            toast(
              resume.missing
                ? '上次照片已不在当前目录，已恢复照片列表；评分仍保存在资料库。'
                : '已接着上次的位置继续挑选',
            );
          }
        }
        if (event.kind === 'cancelled') {
          scanCompleted = false;
          setScanning(false);
          setScanOutcome('cancelled');
        }
      });
      // A rejected mutation reply may follow a committed transaction. Only a
      // complete, successful reread of that same folder resolves the unknown
      // result; opening the picker, a partial scan or a retry must not do so.
      if (
        scanCompleted &&
        scanGeneration.current === generation &&
        reviewWriteError.current &&
        chosen.id === reviewErrorFolder.current
      ) {
        const loaded = new Set(scanned.map((asset) => asset.id));
        if (reviewErrorIds.current.every((id) => loaded.has(id))) {
          reviewWriteError.current = '';
          reviewErrorFolder.current = null;
          reviewErrorIds.current = [];
          setReviewFailure('');
          toast('已读取最新评分并清空本次撤销记录。未重复执行评分或撤销。');
        } else {
          setError('此次操作的部分照片未能载入，尚未完成核对；请恢复原文件后重新扫描。');
        }
      }
    } catch (e) {
      setError(String(e));
      setScanning(false);
      if (scanStarted) setScanOutcome('failed');
    } finally {
      setLoading(false);
    }
  };
  useEffect(() => {
    let stale = false;
    if (native)
      void (async () => {
        try {
          const saved = await bridge.loadWorkspace();
          if (stale) return;
          if (saved) {
            setTheme(saved.view.theme);
            setSize(saved.view.size);
            setAutoAdvance(saved.view.autoAdvance);
            setSort(saved.view.sort);
            if (saved.folderId !== null) {
              const chosen = await bridge.resumeWorkspace();
              if (stale) return;
              if (chosen) await open(false, chosen, saved.view);
            }
          }
        } catch (e) {
          if (!stale) setError(String(e));
        } finally {
          if (!stale) {
            setLoading(false);
            setWorkspaceReady(true);
          }
        }
      })();
    return () => {
      stale = true;
    };
  }, []);
  useLayoutEffect(() => {
    if (
      !native ||
      !workspaceReady ||
      loading ||
      scanning ||
      saving ||
      !folder ||
      scanOutcome !== 'complete'
    )
      return;
    const checkpoint = {
      folderId: folder.id,
      view: {
        mode: mode === 'review' ? ('review' as const) : ('grid' as const),
        active,
        filter,
        sort,
        search,
        size,
        autoAdvance,
        theme,
        offset: gridOffset,
      },
    };
    setWorkspaceStatus('正在保存审片位置…');
    setWorkspaceError('');
    const ticket = ++workspaceRevision.current;
    void workspaceWrites.current
      .save(checkpoint)
      .then(() => {
        if (ticket !== workspaceRevision.current) return;
        setWorkspaceStatus('审片位置已保存');
        setWorkspaceError('');
      })
      .catch((e) => {
        if (ticket !== workspaceRevision.current) return;
        setWorkspaceStatus('审片位置未保存');
        setWorkspaceError(`审片位置未保存：${String(e)}。评分保存独立处理。`);
      });
  }, [
    workspaceReady,
    loading,
    scanning,
    saving,
    folder,
    scanOutcome,
    mode,
    active,
    filter,
    sort,
    search,
    size,
    autoAdvance,
    theme,
    gridOffset,
  ]);
  // Release a rating/undo only after its resulting React state was committed
  // and the layout effect above queued that exact review checkpoint.
  useLayoutEffect(() => {
    if (!saving && reviewWrite.current) {
      saveLock.current = false;
      reviewWrite.current.finish();
      reviewWrite.current = null;
    }
  });
  const beginReviewWrite = () => {
    let finish!: () => void;
    reviewWrite.current = {
      done: new Promise<void>((resolve) => {
        finish = resolve;
      }),
      finish: () => finish(),
    };
    reviewWriteError.current = '';
    setReviewInputHint('');
    saveLock.current = true;
    setSaving(true);
  };
  const flushReview = async (retry: boolean) => {
    while (reviewWrite.current) await reviewWrite.current.done;
    const failures: string[] = [];
    if (reviewWriteError.current)
      failures.push(
        `评分或撤销结果尚未确认：${reviewWriteError.current}。请返回应用，重新扫描当前文件夹核对；不会自动重复评分或撤销。`,
      );
    try {
      await workspaceWrites.current.flush(retry);
      if (retry && workspaceRevision.current > 0) {
        setWorkspaceStatus('审片位置已保存');
        setWorkspaceError('');
      }
    } catch (e) {
      failures.push(`审片位置未保存：${String(e)}`);
    }
    if (failures.length) throw Error(failures.join('\n'));
  };
  const select = (id: string, extend: boolean, range: boolean) => {
    if (range && active) {
      const start = visible.findIndex((a) => a.id === active);
      const end = visible.findIndex((a) => a.id === id);
      setSelected(
        start < 0
          ? [id]
          : visible.slice(Math.min(start, end), Math.max(start, end) + 1).map((a) => a.id),
      );
    } else if (extend)
      setSelected((current) =>
        current.includes(id) ? current.filter((i) => i !== id) : [...current, id],
      );
    else setSelected([id]);
    setActive(id);
  };
  const startReview = (id: string) => {
    setReviewQueue(visible.map((a) => a.id));
    setActive(id);
    setMode('review');
    setDetail(false);
    setViewports({});
  };
  const startCompare = () => {
    if (selected.length < 2 || selected.length > 8) return;
    const ids = visible.filter((a) => selected.includes(a.id)).map((a) => a.id);
    if (ids.length < 2) {
      toast('请在当前结果中选择至少 2 张照片');
      return;
    }
    setCompareIds(ids);
    setReviewQueue(visible.map((a) => a.id));
    setActive(ids[0]);
    setMode('compare');
    setDetail(false);
    setLinkedViewport(fit);
    setViewports({});
  };
  const returnToGrid = () => {
    // Do not broaden the live filter to find a photo that just left it.
    gridReturnTarget.current = visible.some((asset) => asset.id === active) ? active : null;
    setMode('grid');
    setDetail(false);
  };
  const move = (delta: number) => {
    const queue =
      mode === 'grid' ? visible.map((a) => a.id) : mode === 'review' ? reviewQueue : compareIds;
    const id = nextId(queue, active || queue[0], delta);
    setActive(id);
    if (mode === 'grid') setSelected([id]);
    setDetail(false);
    setViewports({});
  };
  const previousCandidate = nextComparisonId(reviewQueue, compareIds, active, -1);
  const followingCandidate = nextComparisonId(reviewQueue, compareIds, active, 1);
  const canReplace = mode === 'compare' && !detail && !scanning && !loading;
  const replaceCandidate = (delta: -1 | 1) => {
    if (mode !== 'compare' || detail || scanning || loading) return;
    const id = nextComparisonId(reviewQueue, compareIds, active, delta);
    if (!id || !byId.has(id)) return;
    const group = compareIds.map((shown) => (shown === active ? id : shown));
    setCompareIds(group);
    setSelected(group);
    setActive(id);
    // Linked zoom intentionally carries the same relative framing to the new
    // candidate; independent panes keep their framing while the new one fits.
    setViewports((current) => ({ ...current, [id]: fit }));
  };
  const rate = async (rating: Rating, explicitId?: string) => {
    if (saveLock.current) {
      setReviewInputHint('刚才的评分未执行：输入时上一笔操作仍在保存。请确认当前照片后重新评分。');
      return;
    }
    if (!folder || scanning || loading || reviewWriteError.current) return;
    const ids = explicitId ? [explicitId] : mode === 'grid' ? selected : [active];
    if (!ids.length || !ids[0]) return;
    beginReviewWrite();
    setError('');
    const before: History = {
      token: '',
      targets: [...ids],
      active: explicitId ?? active,
      queue: [...reviewQueue],
      compare: [...compareIds],
      mode,
      selection: [...selected],
      filter,
      search,
      sort,
    };
    const folderId = folder?.id;
    try {
      const batch = await bridge.rate(ids, rating);
      setAssets((current) => applyChanges(current, batch.changes));
      setHistory((current) => [...current, { ...before, token: batch.token }]);
      if (
        mode === 'review' &&
        snapshot.current.autoAdvance &&
        snapshot.current.active === before.active &&
        snapshot.current.mode === 'review' &&
        snapshot.current.folder?.id === folderId
      ) {
        setActive(nextId(before.queue, before.active));
        setDetail(false);
        setViewports({});
      }
      toast(
        `${ids.length > 1 ? `${ids.length} 张照片 · ` : ''}${ratingLabel(rating)}${native ? ' · 已保存' : ''}`,
      );
    } catch (e) {
      reviewWriteError.current = String(e);
      reviewErrorFolder.current = folderId ?? null;
      reviewErrorIds.current = [...ids];
      setReviewFailure(String(e));
    } finally {
      setSaving(false);
    }
  };
  const undo = async () => {
    if (saveLock.current) {
      setReviewInputHint('刚才的撤销未执行：输入时上一笔操作仍在保存。请保存完成后重新撤销。');
      return;
    }
    const entry = history.at(-1);
    if (!folder || !entry || scanning || loading || reviewWriteError.current) return;
    const folderId = folder.id;
    beginReviewWrite();
    try {
      const batch = await bridge.undo(entry.token);
      setAssets((current) => applyChanges(current, batch.changes));
      setHistory((current) => current.slice(0, -1));
      setActive(entry.active);
      setReviewQueue(entry.queue);
      setCompareIds(entry.compare);
      setMode(entry.mode);
      setSelected(entry.selection);
      setFilter(entry.filter);
      setSearch(entry.search);
      setSort(entry.sort);
      setDetail(false);
      setViewports({});
      toast('已撤销评分，回到上一张照片');
    } catch (e) {
      reviewWriteError.current = String(e);
      reviewErrorFolder.current = folderId;
      reviewErrorIds.current = [...entry.targets];
      setReviewFailure(String(e));
    } finally {
      setSaving(false);
    }
  };
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.isComposing || isTyping(e.target)) return;
      if (
        guideOpen ||
        backupsOpen ||
        syncOpen ||
        relocationOpen ||
        exportOpen ||
        watermarkOpen ||
        moveOpen
      )
        return;
      if (help) {
        if (e.key === 'Escape') setHelp(false);
        if (e.key === 'Tab') {
          e.preventDefault();
          document.querySelector<HTMLButtonElement>('.shortcut-dialog button')?.focus();
        }
        return;
      }
      if (e.key === '?' && !e.metaKey && !e.ctrlKey) {
        e.preventDefault();
        setHelp(true);
        return;
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'z') {
        e.preventDefault();
        if (!e.repeat) void undo();
        return;
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        if (detail) setDetail(false);
        else if (mode !== 'grid') returnToGrid();
        else setSelected([]);
        return;
      }
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (/^[0-5]$/.test(e.key)) {
        e.preventDefault();
        if (!e.repeat) void rate(Number(e.key) as Rating);
      } else if (e.key.toLowerCase() === 'u') {
        e.preventDefault();
        if (!e.repeat) void rate(null);
      } else if ((e.key === '[' || e.key === ']') && mode === 'compare') {
        e.preventDefault();
        replaceCandidate(e.key === ']' ? 1 : -1);
      } else if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
        e.preventDefault();
        move(e.key === 'ArrowRight' ? 1 : -1);
      } else if (e.key.toLowerCase() === 'c' && mode === 'grid') {
        e.preventDefault();
        startCompare();
      } else if (e.key === 'Enter' && mode === 'grid' && active) {
        e.preventDefault();
        startReview(active);
      } else if (e.key === ' ' && mode !== 'grid') {
        e.preventDefault();
        setDetail((current) => !current);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  });
  const setViewport = useCallback(
    (id: string, v: Viewport) => {
      if (linked && mode === 'compare' && !detail) setLinkedViewport(v);
      else setViewports((current) => ({ ...current, [id]: v }));
    },
    [linked, mode, detail],
  );
  const activeView =
    linked && mode === 'compare' && !detail ? linkedViewport : (viewports[active] ?? fit);
  const changeZoom = (factor: number) =>
    setViewport(active, {
      ...activeView,
      scale: Math.max(1, Math.min(6, activeView.scale * factor)),
    });
  const current = byId.get(active);
  const shown =
    mode === 'compare' && !detail
      ? compareIds.map((id) => byId.get(id)).filter((a): a is Asset => Boolean(a))
      : current
        ? [current]
        : [];
  const canCompare =
    selected.filter((id) => visible.some((a) => a.id === id)).length >= 2 && selected.length <= 8;
  const exportAssets =
    mode === 'grid'
      ? selected.length
        ? selected.map((id) => byId.get(id)).filter((a): a is Asset => Boolean(a))
        : visible
      : shown;
  const exportScope =
    mode === 'grid'
      ? selected.length
        ? '已选照片'
        : '当前筛选结果'
      : mode === 'compare' && !detail
        ? '这组比较照片'
        : '当前审片照片';
  const title =
    filter === 'unreviewed'
      ? '还未评分的时刻'
      : filter === 'picks'
        ? '值得珍藏的时刻'
        : filter === 'reject'
          ? '再看一眼，再决定'
          : '所有照片';

  return (
    <div className="app-shell">
      <ExitSaveGuard flushReview={flushReview} />
      <aside
        className="sidebar"
        inert={
          guideOpen ||
          help ||
          backupsOpen ||
          syncOpen ||
          relocationOpen ||
          exportOpen ||
          watermarkOpen ||
          moveOpen
        }
      >
        <div className="brand">
          <span className="brand-mark">
            <Leaf size={22} />
          </span>
          <div>
            Moment Grove<small>留住值得的时刻</small>
            <ApplicationBadge
              onOpen={(button) => {
                guideOrigin.current = button;
                setGuideOpen(true);
              }}
            />
          </div>
        </div>
        <div className="sidebar-label">我的照片</div>
        <nav aria-label="资料库">
          <button
            className={filter === 'all' ? 'nav-item active' : 'nav-item'}
            onClick={() => {
              setFilter('all');
              setMode('grid');
            }}
          >
            <Grid2X2 size={18} />
            所有照片<span>{assets.length || '—'}</span>
          </button>
          <button
            className={filter === 'unreviewed' ? 'nav-item active' : 'nav-item'}
            onClick={() => {
              setFilter('unreviewed');
              setMode('grid');
            }}
          >
            <Circle size={18} />
            未评分<span>{assets.length ? unreviewed : '—'}</span>
          </button>
          <button
            className={filter === 'picks' ? 'nav-item active' : 'nav-item'}
            onClick={() => {
              setFilter('picks');
              setMode('grid');
            }}
          >
            <Star size={18} />
            家庭精选<span>{assets.length ? picks : '—'}</span>
          </button>
          <button
            className={filter === 'reject' ? 'nav-item active' : 'nav-item'}
            onClick={() => {
              setFilter('reject');
              setMode('grid');
            }}
          >
            <CircleSlash size={18} />
            淘汰候选
          </button>
        </nav>
        <div className="sidebar-label folder-label">
          文件夹
          <IconButton
            label="打开照片文件夹"
            disabled={!native || loading || saving || Boolean(reviewFailure)}
            onClick={() => void open(false)}
          >
            <Plus size={14} />
          </IconButton>
        </div>
        {folder ? (
          <div className="folder-item">
            <FolderOpen size={16} />
            <span>{folder.demo ? '山海之间' : folder.name}</span>
            <span className="folder-dot" />
          </div>
        ) : (
          <p className="sidebar-empty">从一个本地文件夹开始</p>
        )}
        <div className="sidebar-note">
          <span className="note-line" />
          <p>
            慢慢挑选，
            <br />
            让美好的回忆留下来。
          </p>
        </div>
        <div className="sidebar-bottom">
          <div className="local-badge">
            <ShieldCheck size={15} />
            <span>本地优先 · 原图不改写</span>
          </div>
          <div className="sidebar-controls">
            <button onClick={() => setHelp(true)}>
              <Keyboard size={16} />
              快捷键
            </button>
            <IconButton
              label="使用说明与资料"
              buttonRef={guideReturnFocus}
              onClick={() => {
                guideOrigin.current = guideReturnFocus.current;
                setGuideOpen(true);
              }}
            >
              <BookOpen size={16} />
            </IconButton>
            <IconButton
              label={theme === 'dark' ? '切换浅色主题' : '切换暗色主题'}
              onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}
            >
              {theme === 'dark' ? <Sun size={17} /> : <Moon size={17} />}
            </IconButton>
          </div>
        </div>
      </aside>
      <main
        className="main-area"
        inert={
          guideOpen ||
          help ||
          backupsOpen ||
          syncOpen ||
          relocationOpen ||
          exportOpen ||
          watermarkOpen ||
          moveOpen
        }
      >
        <header className="topbar">
          <div className="breadcrumb">
            <span>资料库</span>
            <ChevronRight size={13} />
            <strong>
              {folder ? (folder.demo ? '山海之间' : folder.name) : '欢迎来到 Moment Grove'}
            </strong>
            {folder?.demo && <span className="demo-tag">生成示例</span>}
          </div>
          <div className="top-actions">
            {!native && <span className="preview-badge">浏览器交互预览 · 评分不存盘</span>}
            <button
              className="button subtle"
              aria-label="导出照片副本"
              ref={exportReturnFocus}
              disabled={!folder || loading || scanning || saving || Boolean(reviewFailure)}
              onClick={() => setExportOpen(true)}
            >
              <Copy size={15} />
              导出副本
            </button>
            <IconButton
              label="安全整理"
              buttonRef={moveReturnFocus}
              onClick={() => setMoveOpen(true)}
              disabled={loading || scanning || saving || Boolean(reviewFailure)}
            >
              <FolderInput size={17} />
            </IconButton>
            <IconButton
              label="日期城市水印"
              buttonRef={watermarkReturnFocus}
              onClick={() => setWatermarkOpen(true)}
              disabled={!folder || loading || scanning || saving || Boolean(reviewFailure)}
            >
              <CalendarDays size={17} />
            </IconButton>
            <IconButton
              label="重新关联目录"
              buttonRef={relocationReturnFocus}
              onClick={() => setRelocationOpen(true)}
              disabled={loading || scanning || saving || Boolean(reviewFailure)}
            >
              <Link2 size={17} />
            </IconButton>
            <IconButton
              label="评分同步"
              buttonRef={syncReturnFocus}
              onClick={() => setSyncOpen(true)}
              disabled={!folder || loading || scanning || saving || Boolean(reviewFailure)}
            >
              <FileCheck2 size={17} />
            </IconButton>
            <IconButton
              label="资料库备份"
              buttonRef={backupReturnFocus}
              onClick={() => setBackupsOpen(true)}
              disabled={loading || scanning || saving || Boolean(reviewFailure)}
            >
              <Archive size={17} />
            </IconButton>
            <IconButton
              label="撤销评分"
              onClick={() => void undo()}
              disabled={!history.length || saving || scanning || Boolean(reviewFailure)}
            >
              <Undo2 size={17} />
            </IconButton>
            <button
              className="button subtle"
              onClick={() => void open(false)}
              disabled={!native || loading || saving || Boolean(reviewFailure)}
            >
              <FolderOpen size={15} />
              打开文件夹
            </button>
            {folder && (
              <IconButton
                label="重新扫描当前文件夹"
                disabled={loading || scanning || saving}
                onClick={() => void open(false, folder)}
              >
                <RotateCcw size={17} />
              </IconButton>
            )}
          </div>
        </header>
        {(error || reviewFailure) && (
          <div className="error-banner" role="alert">
            <span>
              {reviewFailure && (
                <>
                  这次评分或撤销的保存结果未确认：{reviewFailure}。先重新扫描当前文件夹核对，
                  不会重复执行评分或撤销。重新扫描会回到照片列表并清空本次撤销记录。
                  {error && <br />}
                </>
              )}
              {error}
            </span>
            {reviewFailure ? (
              <button
                className="button subtle"
                disabled={!folder || loading || scanning || saving}
                onClick={() => folder && void open(false, folder)}
              >
                {loading || scanning ? '正在核对评分…' : '重新扫描核对评分'}
              </button>
            ) : (
              <IconButton label="关闭错误提示" onClick={() => setError('')}>
                <X size={14} />
              </IconButton>
            )}
          </div>
        )}
        {workspaceError && (
          <div className="error-banner" role="alert">
            {workspaceError}
          </div>
        )}
        {scanSummary && Object.values(scanSummary.skipped).some((n) => n > 0) && (
          <div className="scan-scope" role="note" aria-label="扫描范围">
            <p>
              已载入 {scanSummary.count} 张照片；
              {[
                scanSummary.skipped.videos ? `视频 ${scanSummary.skipped.videos} 个` : '',
                scanSummary.skipped.raw ? `RAW ${scanSummary.skipped.raw} 个` : '',
                scanSummary.skipped.images ? `不支持的图片 ${scanSummary.skipped.images} 张` : '',
              ]
                .filter(Boolean)
                .join('、')}{' '}
              未载入。
            </p>
            <small>当前只审阅支持的静态照片，跳过的文件保留原位。</small>
          </div>
        )}
        {warnings.length > 0 && (
          <details className="warning-banner">
            <summary>有 {warnings.length} 项需要留意</summary>
            {warnings.map((w, i) => (
              <p key={i}>{w}</p>
            ))}
          </details>
        )}
        {!folder ? (
          <section className="welcome">
            <div className="welcome-art" aria-hidden="true">
              <div className="art-card art-one">
                <div className="art-sun" />
                <div className="art-mountain back" />
                <div className="art-mountain front" />
              </div>
              <div className="art-card art-two">
                <div className="art-sun" />
                <div className="art-mountain back" />
                <div className="art-mountain front" />
              </div>
              <div className="art-card art-three">
                <div className="art-sun" />
                <div className="art-mountain back" />
                <div className="art-mountain front" />
              </div>
              <span className="art-star">
                <Star size={15} fill="currentColor" /> 值得珍藏
              </span>
            </div>
            <span className="eyebrow">A LITTLE SPACE FOR YOUR MEMORIES</span>
            <h1>给回忆，一点留白。</h1>
            <p>
              轻松浏览，认真挑选。
              <br />
              让每一个值得留下的瞬间，都有自己的位置。
            </p>
            <div className="welcome-actions">
              <button
                className="button primary large"
                disabled={!native || loading}
                onClick={() => void open(false)}
              >
                <FolderOpen size={17} />
                打开照片文件夹
              </button>
              <button
                className="button subtle large"
                disabled={loading}
                onClick={() => void open(true)}
              >
                {loading ? <LoaderCircle size={16} className="spin" /> : <Sparkles size={16} />}
                体验示例图库
                <ArrowRight size={15} />
              </button>
            </div>
            <div className="welcome-foot">
              <span>
                <ShieldCheck size={14} />
                原始照片始终保持不变
              </span>
              <span>无需导入 · 无需账号</span>
            </div>
          </section>
        ) : mode === 'grid' ? (
          <>
            <section className="collection-heading">
              <div>
                <span className="eyebrow">YOUR MOMENTS, AT YOUR PACE</span>
                <h1>
                  {title}
                  <span>{visible.length.toLocaleString()}</span>
                </h1>
                <p>
                  {filter === 'picks'
                    ? '4–5 星自动归入家庭精选，不额外占用照片空间。需要时再导出副本。'
                    : folder.demo
                      ? '用一组生成的山海影像，熟悉你的筛片节奏。'
                      : '每一张照片，都值得从容地看一眼。'}
                </p>
              </div>
              <div className="review-progress">
                <span>
                  {assets.length
                    ? Math.round(((assets.length - unreviewed) / assets.length) * 100)
                    : 0}
                  <small>%</small>
                </span>
                <div>
                  已完成评分
                  <div className="progress-track">
                    <i
                      style={{
                        width: `${assets.length ? ((assets.length - unreviewed) / assets.length) * 100 : 0}%`,
                      }}
                    />
                  </div>
                </div>
              </div>
            </section>
            <div className="gallery-toolbar">
              <label className="search-field">
                <Search size={16} />
                <input
                  aria-label="搜索照片"
                  placeholder="搜索照片名称…"
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                />
                <kbd>⌕</kbd>
              </label>
              <div className="toolbar-right">
                <label className="select-field">
                  <SlidersHorizontal size={15} />
                  <select
                    aria-label="评分筛选"
                    value={filter}
                    onChange={(e) => setFilter(e.target.value as Filter)}
                  >
                    <option value="all">全部评分</option>
                    <option value="unreviewed">未评分</option>
                    <option value="picks">4–5 星精选</option>
                    <option value="0">0 · 淘汰候选</option>
                    {[1, 2, 3, 4, 5].map((r) => (
                      <option key={r} value={r}>
                        {r} 星
                      </option>
                    ))}
                  </select>
                </label>
                <label className="select-field">
                  <ArrowDownWideNarrow size={15} />
                  <select
                    aria-label="排序方式"
                    value={sort}
                    onChange={(e) => setSort(e.target.value as Sort)}
                  >
                    <option value="name">名称</option>
                    <option value="date">时间</option>
                    <option value="rating">评分</option>
                  </select>
                </label>
                <span className="toolbar-divider" />
                <input
                  className="density-slider"
                  aria-label="缩略图大小"
                  type="range"
                  min="200"
                  max="380"
                  value={size}
                  onChange={(e) => setSize(Number(e.target.value))}
                />
              </div>
            </div>
            <div className="gallery-container">
              {visible.length ? (
                <Gallery
                  assets={visible}
                  selected={selected}
                  active={active}
                  size={size}
                  initialOffset={scrollOffset.current}
                  restorePosition={gridRestorePosition}
                  onOffsetRestored={clearGridRestore}
                  revealId={gridReturnTarget.current}
                  onRevealed={() => {
                    gridReturnTarget.current = null;
                  }}
                  onOffset={(n) => {
                    scrollOffset.current = n;
                    setGridOffset(n);
                  }}
                  onSelect={select}
                  onOpen={startReview}
                />
              ) : (
                <div className="empty-results">
                  {scanning ? <LoaderCircle size={30} className="spin" /> : <Search size={30} />}
                  <h2>
                    {scanning
                      ? '正在寻找照片'
                      : scanOutcome === 'failed'
                        ? '扫描未完成'
                        : scanOutcome === 'cancelled'
                          ? '扫描已停止'
                          : assets.length
                            ? '没有符合条件的照片'
                            : '未找到支持的照片'}
                  </h2>
                  <p>
                    {scanning
                      ? '第一批照片准备好后就会出现。'
                      : scanOutcome === 'failed'
                        ? '请查看上方提示，恢复目录后重新扫描；需要授权时请重新打开文件夹。'
                        : scanOutcome === 'cancelled'
                          ? '当前结果可能不完整，可点击右上角重新扫描。'
                          : assets.length
                            ? '试试其他评分，或换一个搜索词。'
                            : '当前支持 JPEG、PNG、WebP、HEIC 和 HEIF。可重新扫描或打开其他文件夹。'}
                  </p>
                </div>
              )}
            </div>
            <footer className="statusbar">
              <span>
                {scanning ? (
                  <>
                    <LoaderCircle size={12} className="spin" />
                    扫描中 · 已找到 {assets.length} 张
                    <button
                      className="cancel-scan"
                      onClick={() => void bridge.cancel().catch((e) => setError(String(e)))}
                    >
                      停止扫描
                    </button>
                  </>
                ) : (
                  <>
                    <span className="status-dot" />
                    {scanOutcome === 'cancelled'
                      ? '扫描已停止 · '
                      : scanOutcome === 'failed'
                        ? '扫描未完成 · '
                        : ''}
                    {visible.length} 张照片
                    {selected.length ? ` · 已选择 ${selected.length} 张` : ''}
                  </>
                )}
              </span>
              <span>
                双击审片
                <span className="status-separator" />
                0–5 评分
                <span className="status-separator" />C 比较
              </span>
            </footer>
            {selected.length > 0 && (
              <div className="selection-bar">
                <span className="selection-count">{selected.length}</span>
                <strong>张照片已选择</strong>
                <div className="selection-rating">
                  <RatingButtons
                    value={selected.length === 1 ? (byId.get(selected[0])?.rating ?? null) : null}
                    onRate={(r) => void rate(r)}
                    disabled={saving || scanning || Boolean(reviewFailure)}
                  />
                </div>
                <button className="button primary" onClick={startCompare} disabled={!canCompare}>
                  <Columns2 size={15} />
                  比较{selected.length > 8 ? '（最多 8 张）' : ''}
                  <kbd>C</kbd>
                </button>
                <IconButton label="取消选择" onClick={() => setSelected([])}>
                  <X size={16} />
                </IconButton>
              </div>
            )}
          </>
        ) : (
          <section className="review-workspace">
            <div className="review-toolbar">
              <button className="button subtle" onClick={returnToGrid}>
                <ArrowLeft size={16} />
                返回照片
              </button>
              <span className="view-mode-label">
                {mode === 'compare' ? `${compareIds.length} 图比较` : '专注审片'}
                {' · '}
                <span title="预览统一显示为 SDR；不改变原件或保留 HDR 显示效果">SDR 预览</span>
                {detail ? ' · 细节' : ''}
              </span>
              <div className="review-tools">
                {mode === 'compare' && (
                  <IconButton
                    label={linked ? '关闭同步缩放' : '开启同步缩放'}
                    active={linked}
                    onClick={() => {
                      setLinked(!linked);
                      setLinkedViewport(fit);
                      setViewports({});
                    }}
                  >
                    <Link2 size={16} />
                  </IconButton>
                )}
                <IconButton label="缩小" onClick={() => changeZoom(1 / 1.25)}>
                  <Minus size={15} />
                </IconButton>
                <button
                  className="zoom-label"
                  title="恢复适应窗口"
                  onClick={() => setViewport(active, fit)}
                >
                  {Math.round(activeView.scale * 100)}%
                </button>
                <IconButton label="放大" onClick={() => changeZoom(1.25)}>
                  <Plus size={15} />
                </IconButton>
                <IconButton label="适应窗口" onClick={() => setViewport(active, fit)}>
                  <RotateCcw size={15} />
                </IconButton>
                <IconButton
                  label={detail ? '退出细节查看' : '查看当前照片细节'}
                  active={detail}
                  onClick={() => {
                    setDetail(!detail);
                    setViewports({});
                  }}
                >
                  <Maximize2 size={16} />
                </IconButton>
              </div>
            </div>
            {mode === 'compare' && (
              <div className="compare-replace-bar" role="group" aria-label="连续比较">
                <div className="compare-replace-scope">
                  <span title={byId.get(active)?.relative}>
                    活动照片 · {byId.get(active)?.name}
                  </span>
                  <small>
                    只替换活动照片，其他照片保留 · 列表 {reviewQueue.indexOf(active) + 1} /{' '}
                    {reviewQueue.length}
                  </small>
                </div>
                <button
                  className="button subtle"
                  aria-label="上一候选"
                  disabled={!canReplace || !previousCandidate}
                  onClick={() => replaceCandidate(-1)}
                >
                  <ChevronLeft size={14} />
                  上一候选<kbd>[</kbd>
                </button>
                <button
                  className="button subtle"
                  aria-label="下一候选"
                  disabled={!canReplace || !followingCandidate}
                  onClick={() => replaceCandidate(1)}
                >
                  下一候选
                  <ChevronRight size={14} />
                  <kbd>]</kbd>
                </button>
              </div>
            )}
            <div
              className={`comparison-stage ${shown.length === 1 ? 'single-stage' : ''}`}
              style={{
                gridTemplateColumns: `repeat(${shown.length <= 2 ? shown.length : shown.length <= 4 ? 2 : shown.length <= 6 ? 3 : 4},minmax(0,1fr))`,
                gridTemplateRows: `repeat(${shown.length <= 2 ? 1 : 2},minmax(0,1fr))`,
              }}
            >
              {shown.map((asset) => (
                <PhotoPane
                  key={`${asset.id}-${detail}`}
                  asset={asset}
                  active={active === asset.id}
                  viewport={
                    linked && mode === 'compare' && !detail
                      ? linkedViewport
                      : (viewports[asset.id] ?? fit)
                  }
                  onViewport={(v) => setViewport(asset.id, v)}
                  onActivate={() => setActive(asset.id)}
                  onFocus={() => {
                    setActive(asset.id);
                    setDetail((v) => !v);
                    setViewports({});
                  }}
                  detail={detail}
                  onRate={(r) => void rate(r, asset.id)}
                  disabled={saving || scanning || Boolean(reviewFailure)}
                />
              ))}
            </div>
            <div className="review-bottom">
              <div className="review-navigation">
                <IconButton label="上一张" onClick={() => move(-1)}>
                  <ChevronLeft size={20} />
                </IconButton>
                <span>
                  {(mode === 'compare' ? compareIds : reviewQueue).indexOf(active) + 1}
                  <small> / {(mode === 'compare' ? compareIds : reviewQueue).length}</small>
                </span>
                <IconButton label="下一张" onClick={() => move(1)}>
                  <ChevronRight size={20} />
                </IconButton>
              </div>
              <div className="review-hint">
                <kbd>0–5</kbd>评分<kbd>U</kbd>清除<kbd>Space</kbd>细节<kbd>Esc</kbd>返回
              </div>
              {mode === 'review' ? (
                <label className="auto-advance">
                  <input
                    type="checkbox"
                    checked={autoAdvance}
                    onChange={(e) => setAutoAdvance(e.target.checked)}
                  />
                  评分后下一张
                </label>
              ) : (
                <span className="sync-note">{linked ? '同步相对缩放与移动' : '各图独立缩放'}</span>
              )}
            </div>
          </section>
        )}
        {workspaceStatus && (
          <div className="workspace-status" aria-live="polite">
            {workspaceStatus}
          </div>
        )}
        {reviewInputHint && (
          <div className="toast review-input-hint" role="status">
            {reviewInputHint}
          </div>
        )}
        {notice && !reviewInputHint && (
          <div className="toast" role="status">
            <Check size={16} />
            {notice}
          </div>
        )}
        {saving && (
          <div className="save-indicator" role="status">
            <LoaderCircle size={12} className="spin" />
            正在保存评分
          </div>
        )}
      </main>
      {moveOpen && (
        <MoveDialog
          folder={folder}
          assets={exportAssets}
          scope={exportScope}
          onClose={(source) => {
            setMoveOpen(false);
            if (source) void open(false, source);
          }}
        />
      )}
      {watermarkOpen && folder && (
        <WatermarkDialog
          folder={folder}
          assets={exportAssets}
          scope={exportScope}
          onClose={() => setWatermarkOpen(false)}
        />
      )}
      {exportOpen && folder && (
        <ExportDialog
          folder={folder}
          assets={exportAssets}
          scope={exportScope}
          onClose={() => setExportOpen(false)}
        />
      )}
      {relocationOpen && (
        <RelocationDialog
          onClose={() => setRelocationOpen(false)}
          onRelinked={(result) => {
            setRelocationOpen(false);
            toast(`已重新关联 ${result.folder.name}，关联前状态已备份。`);
            void open(false, result.folder);
          }}
        />
      )}
      {syncOpen && folder && (
        <SyncDialog
          folder={folder}
          onClose={() => setSyncOpen(false)}
          onAdopted={(batch) => {
            setAssets((current) => applyChanges(current, batch.changes));
            setHistory((current) => [
              ...current,
              {
                token: batch.token,
                targets: batch.changes.map((change) => change.id),
                active,
                queue: [...reviewQueue],
                compare: [...compareIds],
                mode,
                selection: [...selected],
                filter,
                search,
                sort,
              },
            ]);
          }}
        />
      )}
      {backupsOpen && (
        <BackupDialog
          onClose={() => setBackupsOpen(false)}
          onRestored={(result) => {
            scanGeneration.current += 1;
            setFolder(null);
            setAssets([]);
            setSelected([]);
            setActive('');
            setHistory([]);
            setReviewQueue([]);
            setCompareIds([]);
            setMode('grid');
            setDetail(false);
            setFilter('all');
            setSearch('');
            setWarnings([]);
            setError('');
            scrollOffset.current = 0;
            setBackupsOpen(false);
            toast(
              `已恢复 ${result.restored.photos} 条记录，恢复前状态已自动备份。请重新打开照片目录。`,
            );
          }}
        />
      )}
      {guideOpen && <GuideDialog onClose={() => setGuideOpen(false)} />}
      {help && (
        <div className="modal-backdrop" onClick={() => setHelp(false)}>
          <section
            className="shortcut-dialog"
            role="dialog"
            aria-modal="true"
            aria-label="快捷键"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="dialog-heading">
              <Keyboard size={22} />
              <h2>手不离开键盘，思路不被打断。</h2>
              <IconButton label="关闭快捷键" onClick={() => setHelp(false)}>
                <X size={18} />
              </IconButton>
            </div>
            <dl>
              {[
                ['0', '标记为淘汰候选'],
                ['1–5', '为当前照片评分'],
                ['U', '恢复未评分'],
                ['← / →', '上一张 / 下一张'],
                ['⌘ / Ctrl + Z', '撤销评分并恢复位置'],
                ['C', '比较已选择的 2–8 张'],
                ['[ / ]', '比较中替换活动照片，其他保留'],
                ['Enter', '进入审片'],
                ['Space', '临时查看细节'],
                ['Esc', '返回上一级'],
              ].map(([key, label]) => (
                <div key={key}>
                  <dt>
                    <kbd>{key}</kbd>
                  </dt>
                  <dd>{label}</dd>
                </div>
              ))}
            </dl>
            <p>网格中可用 ⌘ / Ctrl 多选、Shift 连选。评分不会删除照片。</p>
          </section>
        </div>
      )}
    </div>
  );
}
