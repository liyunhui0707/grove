import { filterAssets, type Asset, type Filter, type Sort } from './domain';

export type ReviewView = {
  mode: 'grid' | 'review';
  active: string;
  filter: Filter;
  sort: Sort;
  search: string;
  size: number;
  autoAdvance: boolean;
  theme: string;
  offset: number;
  branch?: string;
};
export type SavedWorkspace = { folderId: number | null; name: string; view: ReviewView };

export function restoreReview(assets: Asset[], view: ReviewView) {
  const active = assets.find((a) => a.id === view.active);
  // Retain a just-rated active photo even when it left the live filter. Rebuild
  // the remaining queue from current identities, never an old numeric index.
  const visible = filterAssets(assets, view.filter, view.search, view.sort);
  const candidates =
    active && !visible.some((a) => a.id === active.id) ? [...visible, active] : visible;
  const queue = filterAssets(candidates, 'all', '', view.sort).map((a) => a.id);
  return {
    mode: view.mode === 'review' && active ? ('review' as const) : ('grid' as const),
    active: active?.id ?? '',
    queue,
    missing: Boolean(view.active && !active),
  };
}

export function photosInBranch(assets: Asset[], branch: string) {
  return branch ? assets.filter((a) => a.relative.startsWith(branch + '/')) : assets;
}
