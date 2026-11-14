export type Rating = null | 0 | 1 | 2 | 3 | 4 | 5;
export interface Asset {
  id: string;
  folderId: number;
  name: string;
  relative: string;
  width: number;
  height: number;
  bytes: number;
  modified: number;
  captured: string | null;
  rating: Rating;
  revision: number;
  format: string;
}
export interface Folder {
  id: number;
  name: string;
  demo: boolean;
}
export interface Change {
  id: string;
  before: Rating;
  after: Rating;
  revision: number;
}
export interface Batch {
  token: string;
  changes: Change[];
}
export type Filter = 'all' | 'unreviewed' | 'picks' | 'reject' | '0' | '1' | '2' | '3' | '4' | '5';
export type Sort = 'name' | 'date' | 'rating';
export type ScanEvent =
  | { kind: 'batch'; items: Asset[] }
  | { kind: 'done'; count: number; warnings: string[] }
  | { kind: 'cancelled' };
export const ratingLabel = (r: Rating) =>
  r === null ? '未评分' : r === 0 ? '淘汰候选' : `${r} 星`;
export function filterAssets(assets: Asset[], filter: Filter, search: string, sort: Sort): Asset[] {
  const q = search.trim().toLocaleLowerCase();
  return assets
    .filter(
      (a) =>
        (!q || a.relative.toLocaleLowerCase().includes(q)) &&
        (filter === 'all' ||
          (filter === 'unreviewed'
            ? a.rating === null
            : filter === 'picks'
              ? (a.rating ?? -1) >= 4
              : filter === 'reject'
                ? a.rating === 0
                : a.rating === Number(filter))),
    )
    .sort((a, b) => {
      if (sort === 'rating') {
        const diff = (b.rating ?? -1) - (a.rating ?? -1);
        if (diff) return diff;
      }
      if (sort === 'date') {
        const time = (a: Asset) =>
          a.captured
            ? Date.parse(
                a.captured.replace(/^(\d{4}):(\d{2}):(\d{2})/, '$1-$2-$3').replace(' ', 'T'),
              ) || a.modified
            : a.modified;
        const diff = time(b) - time(a);
        if (diff) return diff;
      }
      return (
        a.relative.localeCompare(b.relative, 'zh-CN', { numeric: true }) || a.id.localeCompare(b.id)
      );
    });
}
export function applyChanges(assets: Asset[], changes: Change[]): Asset[] {
  const byId = new Map(changes.map((c) => [c.id, c]));
  return assets.map((a) => {
    const c = byId.get(a.id);
    return c ? { ...a, rating: c.after, revision: c.revision } : a;
  });
}
export function nextId(queue: string[], current: string, delta = 1): string {
  return queue[Math.max(0, Math.min(queue.length - 1, queue.indexOf(current) + delta))] ?? current;
}
export function isTyping(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    Boolean(target.closest('input,textarea,select,[contenteditable="true"],[role="textbox"]'))
  );
}
export function gridColumns(width: number, desired: number): number {
  return Math.max(1, Math.floor(width / desired));
}
