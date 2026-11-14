import { describe, expect, it } from 'vitest';
import { filterAssets, nextId, applyChanges, type Asset, type Rating } from './domain';
const make = (id: string, rating: Rating): Asset => ({
  id,
  rating,
  folderId: 1,
  name: `${id}.jpg`,
  relative: `${id}.jpg`,
  width: 1440,
  height: 960,
  bytes: 100,
  modified: 1,
  captured: null,
  revision: 0,
  format: 'jpg',
});
describe('review semantics', () => {
  it('never conflates unreviewed with zero, including sorting and exact filters', () => {
    const assets = [make('a', null), make('b', 0), make('c', 5)];
    expect(filterAssets(assets, 'unreviewed', '', 'name').map((a) => a.id)).toEqual(['a']);
    expect(filterAssets(assets, '0', '', 'name').map((a) => a.id)).toEqual(['b']);
    expect(filterAssets(assets, 'picks', '', 'name').map((a) => a.id)).toEqual(['c']);
    expect(filterAssets(assets, 'all', '', 'rating').map((a) => a.rating)).toEqual([5, 0, null]);
  });
  it('advances through a captured queue when rating changes a live filter', () => {
    const assets = [make('a', null), make('b', null), make('c', null)];
    const queue = filterAssets(assets, 'unreviewed', '', 'name').map((a) => a.id);
    const updated = applyChanges(assets, [{ id: 'a', before: null, after: 0, revision: 1 }]);
    expect(filterAssets(updated, 'unreviewed', '', 'name').map((a) => a.id)).toEqual(['b', 'c']);
    expect(nextId(queue, 'a')).toBe('b');
    expect(nextId(queue, 'c')).toBe('c');
    expect(nextId(queue, 'a', -1)).toBe('a');
  });
  it('applies asynchronous replies by file ID rather than selection or position', () => {
    const assets = [make('first', null), make('second', null)];
    const updated = applyChanges(assets, [{ id: 'first', before: null, after: 4, revision: 1 }]);
    expect(updated[1].rating).toBeNull();
    expect(updated[0].rating).toBe(4);
    expect(assets[0].rating).toBeNull();
  });
});
