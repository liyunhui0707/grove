import { Channel, convertFileSrc, invoke, isTauri } from '@tauri-apps/api/core';
import type { Asset, Batch, Folder, Rating, ScanEvent } from './domain';
import type { ReviewView, SavedWorkspace } from './workspace';

export const native = isTauri();
export type Snapshot = {
  id: string;
  created: number;
  bytes: number;
  photos: number;
  reviewed: number;
  rejected: number;
};
export type SnapshotList = { items: Snapshot[]; unavailable: number };
export type Restored = { restored: Snapshot; safetyBackup: Snapshot };
export type RelocationEntry = { id: number; name: string; photos: number; pending: number };
export type RelocationReview = {
  token: string;
  oldName: string;
  newName: string;
  photos: number;
  pending: number;
};
export type Relocated = { folder: Folder; safetyBackup: Snapshot };
export type SyncRow = {
  id: string;
  name: string;
  relative: string;
  rating: Rating;
  revision: number;
  diskRating: Rating;
  diskKnown: boolean;
  state: 'ready' | 'conflict' | 'retry' | 'error';
  message: string;
  reviewToken: string | null;
  canRepair: boolean;
};
export type SyncReport = {
  pending: number;
  recoveries: number;
  supported: boolean;
  offset: number;
  items: SyncRow[];
};
export type SyncOutcome = { completed: number; failed: number; report: SyncReport };
export type MoveReport = {
  id: string;
  reviewToken: string | null;
  source: string;
  sourceFolder: number;
  destination: string;
  kind: 'move' | 'quarantine';
  direction: 'forward' | 'restore';
  status: string;
  verified: number;
  items: {
    name: string;
    output: string;
    rating: Rating;
    sidecar: boolean;
    status: string;
    error: string | null;
  }[];
  safetyBackup: string | null;
  message: string | null;
};
export type ExportRow = {
  name: string;
  output: string;
  rating: Rating;
  bytes: number;
  status: string;
  error: string | null;
  caption: string | null;
};
export type WatermarkOptions = {
  city: string;
  fallbackDate: string | null;
  font: 'sans' | 'serif';
  sizePercent: number;
  color: string;
  opacity: number;
  marginPercent: number;
  shadow: boolean;
};
export type WatermarkStyle = Omit<WatermarkOptions, 'city' | 'fallbackDate'>;
export type WatermarkPreview = {
  dataUrl: string;
  canExport: boolean;
  info: {
    source: {
      width: number;
      height: number;
      date: string | null;
      dateSource: string;
      text: string;
      warning: string | null;
    };
    outputWidth: number;
    outputHeight: number;
  };
};
export type ExportReport = {
  id: string;
  destination: string;
  status: string;
  bytes: number;
  verified: number;
  items: ExportRow[];
  message: string | null;
  kind: 'copy' | 'watermark';
  watermark: WatermarkOptions | null;
};
export type PreparationProgress = {
  phase: string;
  checked: number;
  total: number;
  current: string | null;
};
const progressChannel = (onProgress: (p: PreparationProgress) => void) => {
  const channel = new Channel<PreparationProgress>();
  channel.onmessage = onProgress;
  return channel;
};
export const bridge = {
  registerStyleExitGuard: (onRequest: (token: number) => void) => {
    const channel = new Channel<number>();
    channel.onmessage = onRequest;
    return invoke<void>('register_style_exit_guard', { onRequest: channel });
  },
  resolveStyleExit: (token: number, proceed: boolean) =>
    invoke<void>('resolve_style_exit', { token, proceed }),
  loadWatermarkStyle: () => invoke<WatermarkStyle | null>('load_watermark_style'),
  saveWatermarkStyle: (style: WatermarkStyle) => invoke<void>('save_watermark_style', { style }),
  loadWorkspace: () =>
    native ? invoke<SavedWorkspace | null>('load_workspace') : Promise.resolve(null),
  resumeWorkspace: () => invoke<Folder | null>('resume_workspace'),
  saveWorkspace: (folderId: number | null, view: ReviewView) =>
    native ? invoke<void>('save_workspace', { folderId, view }) : Promise.resolve(),
  applicationInfo: () =>
    invoke<{
      version: string;
      buildId: string;
      channel: string;
      dataDirectory: string;
      isolated: boolean;
      executable: string;
    }>('application_info'),
  moveHistory: () => invoke<MoveReport[]>('move_history'),
  chooseMove: (
    folderId: number | null,
    assets: Asset[],
    kind: 'move' | 'quarantine',
    resumeId: string | null = null,
    restore = false,
    onProgress: (p: PreparationProgress) => void = () => {},
  ) =>
    invoke<MoveReport | null>('choose_move_folder', {
      folderId,
      targets: assets.map(({ id, revision }) => ({ id, revision })),
      kind,
      resumeId,
      restore,
      onProgress: progressChannel(onProgress),
    }),
  async confirmMove(token: string, onEvent: (report: MoveReport) => void) {
    const channel = new Channel<MoveReport>();
    channel.onmessage = onEvent;
    return invoke<MoveReport>('confirm_move', { token, onEvent: channel });
  },
  cancelMove: () => invoke<void>('cancel_move'),
  exportHistory: (folderId: number) => invoke<ExportReport[]>('export_history', { folderId }),
  chooseExport: (
    folderId: number,
    assets: Asset[],
    resumeId: string | null = null,
    onProgress: (p: PreparationProgress) => void = () => {},
  ) =>
    invoke<ExportReport | null>('choose_export_folder', {
      folderId,
      targets: assets.map(({ id, revision }) => ({ id, revision })),
      resumeId,
      watermark: null,
      onProgress: progressChannel(onProgress),
    }),
  chooseWatermark: (
    folderId: number,
    assets: Asset[],
    watermark: WatermarkOptions,
    resumeId: string | null = null,
    onProgress: (p: PreparationProgress) => void = () => {},
  ) =>
    invoke<ExportReport | null>('choose_export_folder', {
      folderId,
      targets: assets.map(({ id, revision }) => ({ id, revision })),
      resumeId,
      watermark,
      onProgress: progressChannel(onProgress),
    }),
  previewWatermark: (id: string, options: WatermarkOptions) =>
    invoke<WatermarkPreview>('watermark_preview', { id, options }),
  async confirmExport(token: string, onEvent: (report: ExportReport) => void) {
    const channel = new Channel<ExportReport>();
    channel.onmessage = onEvent;
    return invoke<ExportReport>('confirm_export', { token, onEvent: channel });
  },
  cancelExport: () => invoke<void>('cancel_export'),
  chooseFolder: () => invoke<Folder | null>('choose_folder'),
  demo: () =>
    native
      ? invoke<Folder>('create_demo')
      : Promise.resolve({ id: 1, name: '山海之间 · 交互示例', demo: true }),
  async scan(folderId: number, onEvent: (event: ScanEvent) => void) {
    if (!native) {
      onEvent({ kind: 'batch', items: browserSamples() });
      onEvent({ kind: 'done', count: 24, warnings: [] });
      return;
    }
    const channel = new Channel<ScanEvent>();
    channel.onmessage = onEvent;
    await invoke('scan_folder', { folderId, onEvent: channel });
  },
  rate: (ids: string[], rating: Rating) =>
    native ? invoke<Batch>('set_rating', { ids, rating }) : browserRate(ids, rating),
  undo: (token: string) => (native ? invoke<Batch>('undo_rating', { token }) : browserUndo(token)),
  cancel: () => (native ? invoke('cancel_scan') : Promise.resolve()),
  backups: () => invoke<SnapshotList>('list_backups'),
  createBackup: () => invoke<Snapshot>('create_backup'),
  restoreBackup: (id: string) => invoke<Restored>('restore_backup', { id }),
  relocationFolders: () => invoke<RelocationEntry[]>('list_relocation_folders'),
  chooseRelocation: (folderId: number) =>
    invoke<RelocationReview | null>('choose_relocation_folder', { folderId }),
  confirmRelocation: (token: string) => invoke<Relocated>('confirm_relocation', { token }),
  inspectXmp: (folderId: number, offset = 0) =>
    invoke<SyncReport>('inspect_xmp', { folderId, offset }),
  syncXmp: (folderId: number, rows: SyncRow[]) =>
    invoke<SyncOutcome>('sync_xmp', {
      folderId,
      targets: rows.map(({ id, revision }) => ({ id, revision })),
    }),
  adoptXmp: (folderId: number, row: SyncRow) =>
    invoke<Batch>('adopt_xmp', {
      folderId,
      id: row.id,
      revision: row.revision,
      token: row.reviewToken,
    }),
  repairXmp: (folderId: number, id: string) => invoke<void>('repair_xmp', { folderId, id }),
};
const sampleRatings = new Map<string, Rating>();
const sampleHistory = new Map<string, Batch>();
async function browserRate(ids: string[], rating: Rating): Promise<Batch> {
  const batch = {
    token: crypto.randomUUID(),
    changes: ids.map((id) => ({
      id,
      before: sampleRatings.get(id) ?? null,
      after: rating,
      revision: 1,
    })),
  };
  for (const id of ids) sampleRatings.set(id, rating);
  sampleHistory.set(batch.token, batch);
  return batch;
}
async function browserUndo(token: string): Promise<Batch> {
  const batch = sampleHistory.get(token);
  if (!batch) throw new Error('没有可撤销的评分');
  sampleHistory.delete(token);
  return {
    token,
    changes: batch.changes.map((c) => {
      sampleRatings.set(c.id, c.before);
      return { ...c, after: c.before, before: c.after };
    }),
  };
}
export function mediaSource(asset: Asset, size: 480 | 1600 | 8192, attempt = 0): string {
  return native
    ? `${convertFileSrc(asset.id, 'grove')}?size=${size}&stamp=${asset.modified}${attempt ? `&refresh=1&attempt=${attempt}` : ''}`
    : sampleSource(Number(asset.id.split('-')[1]));
}
// Browser previews are explicitly illustrative. Desktop persistence always goes through SQLite.
export function browserSamples(): Asset[] {
  return Array.from({ length: 24 }, (_, i) => ({
    id: `sample-${i}`,
    folderId: 1,
    name: `山海_${String(i + 1).padStart(2, '0')}.jpg`,
    relative: `山海_${String(i + 1).padStart(2, '0')}.jpg`,
    width: i % 5 === 0 ? 960 : 1440,
    height: i % 5 === 0 ? 1280 : 960,
    bytes: 540000 + i * 9371,
    modified: Date.UTC(2026, 9, 25) + i * 60000,
    captured: null,
    rating: null,
    revision: 0,
    format: 'jpg',
  }));
}
function sampleSource(index: number) {
  const palettes = [
    ['#bbd0ca', '#d9d9c8', '#829b91', '#365e55'],
    ['#bdcbd8', '#e9ded3', '#8d9bad', '#394d68'],
    ['#e0c8a8', '#f1dfbb', '#b3b18c', '#6b774d'],
    ['#c1c9bf', '#dfdcd0', '#7f9a88', '#3d5b4e'],
  ];
  const [sky, sun, far, near] = palettes[index % 4];
  const phase = index % 6;
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1440" height="960" viewBox="0 0 1440 960"><defs><linearGradient id="s" x2="0" y2="1"><stop stop-color="${sky}"/><stop offset="1" stop-color="${sun}"/></linearGradient><filter id="n"><feTurbulence baseFrequency=".8" numOctaves="3" stitchTiles="stitch"/><feColorMatrix type="saturate" values="0"/><feComponentTransfer><feFuncA type="linear" slope=".05"/></feComponentTransfer><feBlend in="SourceGraphic" mode="soft-light"/></filter></defs><g filter="url(#n)"><path fill="url(#s)" d="M0 0h1440v960H0z"/><circle cx="${1030 - phase * 37}" cy="240" r="62" fill="${sun}"/><path fill="${far}" d="M0 640L${220 + phase * 60} 300 650 610 990 ${380 + phase * 30} 1440 590V960H0Z"/><path fill="${near}" opacity=".7" d="M0 710Q400 ${440 + phase * 20} 780 680T1440 640V960H0Z"/><path fill="${near}" d="M0 840Q500 600 1440 870V960H0Z"/></g></svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}
