import { test } from 'node:test';
import assert from 'node:assert/strict';
import { releaseVersion, releaseArchiveStem } from '../release-version.mjs';

const fixture = () => ({
  pkg: { name: 'moment-grove', version: '0.2.0' },
  lock: { version: '0.2.0', packages: { '': { version: '0.2.0' } } },
  tauri: { identifier: 'app.momentgrove.desktop', version: '0.2.0' },
  cargoToml:
    '[package]\nname = "moment-grove"\nversion = "0.2.0"\n\n[dependencies]\nother = "0.1.0"\n',
  cargoLock:
    '[[package]]\nname = "other"\nversion = "0.1.0"\n\n[[package]]\nname = "moment-grove"\nversion = "0.2.0"\n',
});
test('all release manifests agree without using a dependency version', () => {
  assert.equal(releaseVersion(fixture()), '0.2.0');
  assert.equal(
    releaseArchiveStem('0.2.0', 'build-id', 'public-prerelease'),
    'Moment-Grove-0.2.0-macOS-arm64',
  );
  assert.equal(
    releaseArchiveStem('0.2.0', 'build-id', 'local-trial'),
    'Moment-Grove-0.2.0-build-id-macOS-arm64',
  );
});
test('a stale manifest or wrong app identity stops packaging', () => {
  for (const mutate of [
    (f) => {
      f.lock.version = '0.1.0';
    },
    (f) => {
      f.lock.packages[''].version = '0.1.0';
    },
    (f) => {
      f.tauri.version = '0.1.0';
    },
    (f) => {
      f.cargoToml = f.cargoToml.replace('0.2.0', '0.1.0');
    },
    (f) => {
      f.cargoLock = f.cargoLock.replace('0.2.0', '0.1.0');
    },
    (f) => {
      f.tauri.identifier = 'app.other';
    },
  ]) {
    const f = fixture();
    mutate(f);
    assert.throws(() => releaseVersion(f));
  }
});
