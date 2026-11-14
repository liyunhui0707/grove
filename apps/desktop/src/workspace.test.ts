import { describe, expect, it } from 'vitest';
import { restoreReview, type ReviewView } from './workspace';
import type { Asset } from './domain';
const assets = [0, 1, 2, 3].map(
  (i) =>
    ({
      id: `id-${i}`,
      name: `${i}.jpg`,
      relative: `${i}.jpg`,
      rating: i === 2 ? 4 : null,
      folderId: 1,
      modified: i,
      captured: null,
    }) as Asset,
);
const view: ReviewView = {
  mode: 'review',
  active: 'id-2',
  filter: 'unreviewed',
  sort: 'name',
  search: '',
  size: 260,
  autoAdvance: false,
  theme: 'light',
  offset: 0,
};
describe('restart review position', () => {
  it('keeps a rated active occurrence outside live filter and continues in stable order', () => {
    const r = restoreReview(assets, view);
    expect(r.active).toBe('id-2');
    expect(r.queue).toEqual(['id-0', 'id-1', 'id-2', 'id-3']);
    expect(r.mode).toBe('review');
  });
  it('never substitutes another occurrence when saved photo disappears', () => {
    const r = restoreReview(
      assets.filter((a) => a.id !== 'id-2'),
      view,
    );
    expect(r.active).toBe('');
    expect(r.mode).toBe('grid');
    expect(r.missing).toBe(true);
  });
});
