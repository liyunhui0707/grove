import { test } from 'node:test';
import assert from 'node:assert/strict';
import { packagingOptions, signingPreflight, signApp, notarizeTarget } from '../macos-signing.mjs';

const fingerprint = 'A'.repeat(40);
const args = [
  '--prerelease',
  '--developer-id',
  fingerprint,
  '--notary-profile',
  'grove-notary',
  '--notarize',
];
const options = () => packagingOptions(args);
const accepted = { id: '00000000-0000-4000-8000-000000000001', status: 'Accepted' };

test('ad-hoc stays offline and cannot become notarized through partial flags', () => {
  const calls = [];
  const run = (...call) => {
    calls.push(call);
    return '';
  };
  const o = packagingOptions([]);
  assert.equal(signingPreflight(o, run).mode, 'ad-hoc');
  signApp('/qa/App.app', o, run);
  assert.deepEqual(
    calls.map((c) => c[0]),
    ['/usr/bin/codesign', '/usr/bin/codesign'],
  );
  assert.ok(calls[0][1].includes('--timestamp=none'));
  for (const bad of [
    ['--notarize'],
    ['--prerelease', '--developer-id', fingerprint],
    ['--prerelease', '--notary-profile', 'grove'],
    ['--developer-id', fingerprint, '--notary-profile', 'grove', '--notarize'],
    [...args, '--prerelease'],
    ['--prerelease', '--developer-id', '-', '--notary-profile', 'grove', '--notarize'],
  ])
    assert.throws(() => packagingOptions(bad));
});

test('a development certificate or different fingerprint fails before build or upload', () => {
  for (const listing of [
    '',
    `1) ${fingerprint} "Apple Development: Example"`,
    `1) ${'B'.repeat(40)} "Developer ID Application: Example"`,
  ]) {
    const calls = [];
    assert.throws(
      () =>
        signingPreflight(options(), (...call) => {
          calls.push(call);
          return listing;
        }),
      /unavailable/,
    );
    assert.equal(calls.length, 1);
  }
});

test('Developer ID mode checks tools and signs with runtime and secure timestamp', () => {
  const calls = [];
  const run = (...call) => {
    calls.push(call);
    return `1) ${fingerprint} "Developer ID Application: Example"`;
  };
  assert.equal(signingPreflight(options(), run).identityAvailable, true);
  signApp('/qa/App.app', options(), run);
  assert.deepEqual(calls[3][1], [
    '--force',
    '--sign',
    fingerprint,
    '--options',
    'runtime',
    '--timestamp',
    '/qa/App.app',
  ]);
  assert.ok(!calls.flat(3).includes('--timestamp=none'));
});

test('only Accepted can lead to stapling, validation and a completed ticket', () => {
  const calls = [],
    records = [];
  const result = notarizeTarget(
    '/qa/upload.zip',
    '/qa/App.app',
    options(),
    (...call) => {
      calls.push(call);
      return JSON.stringify(accepted);
    },
    (r) => records.push(r),
  );
  assert.equal(result.ticket, 'STAPLED_AND_VALIDATED');
  assert.deepEqual(records, [accepted]);
  assert.deepEqual(
    calls.map((c) => c[1].slice(0, 2)),
    [
      ['notarytool', 'submit'],
      ['stapler', 'staple'],
      ['stapler', 'validate'],
    ],
  );
  assert.ok(calls[0][1].includes('--keychain-profile'));
  assert.ok(!calls[0][1].includes('--password'));
});

test('rejection, malformed output, timeout and stapling errors never downgrade or retry', () => {
  for (const response of [
    JSON.stringify({ ...accepted, status: 'Invalid' }),
    'not-json',
    JSON.stringify({ status: 'Accepted' }),
  ]) {
    const calls = [],
      records = [];
    assert.throws(() =>
      notarizeTarget(
        '/qa/upload.zip',
        '/qa/App.app',
        options(),
        (...call) => {
          calls.push(call);
          return response;
        },
        (r) => records.push(r),
      ),
    );
    assert.equal(calls.length, 1);
    assert.equal(records.length, 1);
  }
  const records = [];
  assert.throws(
    () =>
      notarizeTarget(
        '/qa/a.zip',
        '/qa/a.app',
        options(),
        () => {
          throw Object.assign(Error('timeout'), {
            stdout: JSON.stringify({ ...accepted, status: 'In Progress' }),
          });
        },
        (r) => records.push(r),
      ),
    /incomplete/,
  );
  assert.equal(records[0].id, accepted.id);
  let count = 0;
  assert.throws(
    () =>
      notarizeTarget(
        '/qa/a.zip',
        '/qa/a.app',
        options(),
        () => {
          if (++count === 2) throw Error('stapler unavailable');
          return JSON.stringify(accepted);
        },
        () => {},
      ),
    /stapler unavailable/,
  );
  assert.equal(count, 2);
});
