import assert from 'node:assert/strict';
import test from 'node:test';
import { batchLayout, candidateLayout, exportLayout } from '../qa/candidate-layout.mjs';

test('candidate and export paths follow each supported product', () => {
  for (const [name, binary, prefix] of [
    ['Grove', 'grove', 'Grove'],
    ['Moment Grove', 'moment-grove', 'Moment-Grove'],
  ]) {
    const layout = batchLayout({ product: name });
    assert.equal(layout.app, name + '.app');
    assert.equal(layout.binary, binary);
    for (const kind of ['', 'Watermark-']) {
      assert.equal(
        exportLayout(prefix + '-' + kind + '1790000000-abcdef12')?.portable,
        binary + '-export.json',
      );
    }
  }
  assert.throws(() => candidateLayout('../Grove'));
  assert.equal(exportLayout('Grove-family-photos'), null);
});

test('implicit legacy identity requires the fingerprint of the listed executable', () => {
  const sha = 'f'.repeat(64);
  for (const [key, path] of [
    ['appFiles', 'Contents/MacOS/moment-grove'],
    ['files', 'Moment Grove QA.app/Contents/MacOS/moment-grove'],
  ]) {
    const manifest = { appVersion: '0.1.0', binarySha256: sha, [key]: [{ path, sha256: sha }] };
    assert.equal(batchLayout(manifest).product, 'Moment Grove');
    assert.throws(() => batchLayout({ ...manifest, binarySha256: 'a'.repeat(64) }));
    assert.throws(() => batchLayout({ ...manifest, appVersion: '0.10.0' }));
    assert.throws(() => batchLayout({ ...manifest, product: 'other' }));
  }
});

test('the first native QA manifest uses the source executable fingerprint', () => {
  const sha = 'd'.repeat(64);
  const manifest = {
    appVersion: '0.1.0',
    sourceAppExecutableSha256: sha,
    files: [{ path: 'Moment Grove QA.app/Contents/MacOS/moment-grove', sha256: sha }],
  };
  assert.equal(batchLayout(manifest).product, 'Moment Grove');
  assert.throws(() => batchLayout({ ...manifest, binarySha256: 'a'.repeat(64) }));
  assert.throws(() => batchLayout({ ...manifest, sourceAppExecutableSha256: 'b'.repeat(64) }));
});
