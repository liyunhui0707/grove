import test from 'node:test';
import assert from 'node:assert/strict';
import { trialLaunchArguments } from '../open-desktop.mjs';

test('public download and local trial use the same independent catalog', () => {
  const app = '/test/install/Grove.app';
  const home = '/test/user with spaces';
  // A running daily app with the same bundle ID must not swallow the override.
  const expected = [
    '-n',
    '--env',
    'MOMENT_GROVE_DATA_DIR=/test/user with spaces/Library/Application Support/app.momentgrove.trial',
    app,
  ];
  assert.deepEqual(trialLaunchArguments(app, 'local-trial', home), expected);
  assert.deepEqual(trialLaunchArguments(app, 'public-prerelease', home), expected);
});

test('trial entry rejects unknown channels and relative scope instead of using daily data', () => {
  assert.throws(() => trialLaunchArguments('/test/App.app', 'development', '/test/user'));
  assert.throws(() => trialLaunchArguments('/test/App.app', 'public-prerelease', 'relative'));
  assert.throws(() => trialLaunchArguments('relative.app', 'local-trial', '/test/user'));
});
