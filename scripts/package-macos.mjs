// Local candidate packaging; publication is a separate, explicit operation.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { root } from './toolchain.mjs';
import { releaseVersion, releaseArchiveStem } from './release-version.mjs';
import { packagingOptions, signingPreflight, signApp, notarizeTarget } from './macos-signing.mjs';

if (process.platform !== 'darwin' || process.arch !== 'arm64')
  throw Error('This packaging entry supports the macOS Apple Silicon candidate only');
const options = packagingOptions(process.argv.slice(2));
const { channel } = options;
// Keep maintainer and dependency-cache paths out of publicly distributed binaries.
process.env.CARGO_ENCODED_RUSTFLAGS = [
  ...(process.env.CARGO_ENCODED_RUSTFLAGS?.split('\x1f') ?? []),
  '--remap-path-prefix',
  `${root}=moment-grove`,
  '--remap-path-prefix',
  `${root}/.local=build-cache`,
].join('\x1f');
const sha = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');
const run = (cmd, args) =>
  execFileSync(cmd, args, { cwd: root, encoding: 'utf8', timeout: 600000 });
const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json')));
const config = JSON.parse(
  fs.readFileSync(path.join(root, 'apps/desktop/src-tauri/tauri.conf.json')),
);
releaseVersion({
  pkg,
  lock: JSON.parse(fs.readFileSync(path.join(root, 'package-lock.json'))),
  tauri: config,
  cargoToml: fs.readFileSync(path.join(root, 'apps/desktop/src-tauri/Cargo.toml'), 'utf8'),
  cargoLock: fs.readFileSync(path.join(root, 'apps/desktop/src-tauri/Cargo.lock'), 'utf8'),
});
const signing = signingPreflight(options, run);
if (options.preflight) {
  console.log(
    JSON.stringify(
      {
        ...signing,
        channel,
        version: pkg.version,
        credentialsValidated: false,
        note: 'No build, signing, notarization upload or publication performed.',
      },
      null,
      2,
    ),
  );
  process.exit(0);
}
const sourcePaths = [
  'package.json',
  'package-lock.json',
  'vite.config.ts',
  'tsconfig.json',
  'index.html',
  'public/startup.css',
  'public/startup.js',
  'scripts/package-macos.mjs',
  'scripts/macos-signing.mjs',
  'scripts/release-version.mjs',
  'scripts/toolchain.mjs',
  'scripts/desktop.mjs',
  'apps/desktop/src-tauri/Cargo.toml',
  'apps/desktop/src-tauri/Cargo.lock',
  'apps/desktop/src-tauri/build.rs',
  'apps/desktop/src-tauri/tauri.conf.json',
  ...files(path.join(root, 'apps/desktop/src-tauri/capabilities')).map(
    (p) => 'apps/desktop/src-tauri/capabilities/' + p,
  ),
  ...files(path.join(root, 'apps/desktop/src')).map((p) => 'apps/desktop/src/' + p),
  ...files(path.join(root, 'apps/desktop/src-tauri/src')).map(
    (p) => 'apps/desktop/src-tauri/src/' + p,
  ),
  ...files(path.join(root, 'apps/desktop/src-tauri/icons')).map(
    (p) => 'apps/desktop/src-tauri/icons/' + p,
  ),
];
const sources = sourcePaths.map((p) => ({
  path: p,
  sha256: sha(fs.readFileSync(path.join(root, p))),
}));
const sourceRaw = JSON.stringify(sources, null, 2);

const buildId = `trial-${new Date().toISOString().replace(/[-:]/g, '').slice(0, 15)}-${sha(sourceRaw).slice(0, 10)}`;
process.env.MOMENT_GROVE_BUILD_ID = buildId;
process.env.MOMENT_GROVE_CHANNEL = channel;
const out = path.join(root, '.local', 'deliveries', `macos-${crypto.randomUUID()}`);
fs.mkdirSync(out, { recursive: true, mode: 0o700 });
fs.writeFileSync(path.join(out, 'source-manifest.json'), sourceRaw, { mode: 0o600 });
process.stdout.write(
  run(process.execPath, [
    path.join(root, 'scripts/desktop.mjs'),
    'build',
    '--bundles',
    'app',
    '--no-sign',
    ...(channel === 'local-trial'
      ? ['--config', JSON.stringify({ identifier: 'app.momentgrove.trial' })]
      : []),
    '--',
    '--offline',
  ]),
);
const payload = path.join(out, 'Moment Grove');
fs.mkdirSync(payload, { mode: 0o700 });
const app = path.join(payload, 'Moment Grove.app');
fs.cpSync(
  path.join(root, 'apps/desktop/src-tauri/target/release/bundle/macos/Moment Grove.app'),
  app,
  { recursive: true, errorOnExist: true },
);
signApp(app, options, run);
let appNotarization = { status: 'NOT_REQUESTED' };
if (options.notarize) {
  const upload = path.join(out, 'notarization-app.zip');
  run('/usr/bin/ditto', ['-c', '-k', '--sequesterRsrc', '--keepParent', app, upload]);
  appNotarization = notarizeTarget(upload, app, options, run, (result) =>
    fs.writeFileSync(path.join(out, 'notarization-app.json'), JSON.stringify(result, null, 2), {
      mode: 0o600,
    }),
  );
  run('/usr/bin/codesign', ['--verify', '--deep', '--strict', app]);
  run('/usr/sbin/spctl', ['--assess', '--type', 'execute', app]);
}
const info = JSON.parse(
  run('/usr/bin/plutil', ['-convert', 'json', '-o', '-', path.join(app, 'Contents/Info.plist')]),
);
const binary = path.join(app, 'Contents/MacOS/moment-grove');
if (
  info.CFBundleIdentifier !==
    (channel === 'local-trial' ? 'app.momentgrove.trial' : config.identifier) ||
  info.CFBundleShortVersionString !== pkg.version ||
  run('/usr/bin/lipo', ['-archs', binary]).trim() !== 'arm64'
)
  throw Error('Built bundle identity or architecture mismatch');
const dependencies = run('/usr/bin/otool', ['-L', binary])
  .trim()
  .split('\n')
  .slice(1)
  .map((s) => s.trim().split(' (')[0]);
if (dependencies.some((s) => !s.startsWith('/System/Library/') && !s.startsWith('/usr/lib/')))
  throw Error('Candidate depends on a non-system library');
function files(dir, prefix = '') {
  return fs
    .readdirSync(dir, { withFileTypes: true })
    .sort((a, b) => a.name.localeCompare(b.name))
    .flatMap((e) => {
      const relative = prefix + e.name;
      if (e.isSymbolicLink()) throw Error('Unexpected link in candidate sources');
      return e.isDirectory() ? files(path.join(dir, e.name), relative + '/') : [relative];
    });
}
for (const [from, to] of [
  ['docs/user-guide.md', '使用说明.md'],
  ['docs/format-support.md', 'format-support.md'],
  ['docs/installation-upgrade.md', 'installation-upgrade.md'],
  ['docs/release-notes.md', 'release-notes.md'],
  ['LICENSE', 'LICENSE'],
  ['docs/third-party.md', 'THIRD-PARTY.md'],
  ['THIRD-PARTY-NOTICES.txt', 'THIRD-PARTY-NOTICES.txt'],
])
  fs.copyFileSync(path.join(root, from), path.join(payload, to));
const candidate = {
  product: 'Moment Grove',
  version: pkg.version,
  channel,
  buildId,
  architecture: 'arm64',
  minimumSystemDeclaration: info.LSMinimumSystemVersion,
  testedSystems: [],
  signing: options.notarize
    ? 'Developer ID Application; hardened runtime; secure timestamp'
    : 'ad-hoc; no Developer ID or notarization',
  notarization: appNotarization,
  update: 'manual replacement',
  created: new Date().toISOString(),
  sourceDigest: sha(sourceRaw),
  binarySha256: sha(fs.readFileSync(binary)),
  systemDependencies: dependencies,
};
fs.writeFileSync(path.join(payload, 'candidate.json'), JSON.stringify(candidate, null, 2));
const entries = files(payload).map((p) => ({
  path: p,
  bytes: fs.statSync(path.join(payload, p)).size,
  sha256: sha(fs.readFileSync(path.join(payload, p))),
}));
fs.writeFileSync(
  path.join(payload, 'SHA256SUMS.txt'),
  entries.map((e) => `${e.sha256}  ${e.path}`).join('\n') + '\n',
);
fs.symlinkSync('/Applications', path.join(payload, 'Applications'));
const archiveStem = releaseArchiveStem(pkg.version, buildId, channel);
const zip = path.join(out, `${archiveStem}.zip`);
run('/usr/bin/ditto', ['-c', '-k', '--sequesterRsrc', '--keepParent', payload, zip]);
const dmg = path.join(out, `${archiveStem}.dmg`);
run('/usr/bin/hdiutil', [
  'create',
  '-volname',
  channel === 'public-prerelease' ? `Moment Grove ${pkg.version} Preview` : 'Moment Grove Trial',
  '-srcfolder',
  payload,
  '-format',
  'UDZO',
  dmg,
]);
let dmgNotarization = { status: 'NOT_REQUESTED' };
if (options.notarize) {
  run('/usr/bin/codesign', ['--force', '--sign', options.identity, '--timestamp', dmg]);
  run('/usr/bin/codesign', ['--verify', '--strict', dmg]);
  dmgNotarization = notarizeTarget(dmg, dmg, options, run, (result) =>
    fs.writeFileSync(path.join(out, 'notarization-dmg.json'), JSON.stringify(result, null, 2), {
      mode: 0o600,
    }),
  );
  run('/usr/sbin/spctl', [
    '--assess',
    '--type',
    'open',
    '--context',
    'context:primary-signature',
    dmg,
  ]);
}
const receipt = {
  ...candidate,
  status: 'BUILT_NOT_ACCEPTED',
  directory: out,
  files: entries,
  artifacts: [zip, dmg].map((p) => ({
    name: path.basename(p),
    bytes: fs.statSync(p).size,
    sha256: sha(fs.readFileSync(p)),
  })),
  nativeGuiAcceptance: 'NOT_RUN',
  installationAcceptance: 'NOT_RUN',
  dmgNotarization,
};
fs.writeFileSync(
  path.join(out, 'SHA256SUMS.txt'),
  receipt.artifacts.map((a) => `${a.sha256}  ${a.name}`).join('\n') + '\n',
);
fs.writeFileSync(path.join(out, 'receipt.json'), JSON.stringify(receipt, null, 2), { mode: 0o600 });
fs.writeFileSync(
  path.join(
    root,
    '.local',
    channel === 'public-prerelease' ? 'prerelease-candidate.json' : 'local-trial-candidate.json',
  ),
  JSON.stringify({ directory: out, receipt: path.join(out, 'receipt.json') }, null, 2),
  { mode: 0o600 },
);
console.log(
  JSON.stringify({ status: receipt.status, directory: out, artifacts: receipt.artifacts }, null, 2),
);

if (channel === 'local-trial') {
  console.log(
    'Candidate built; the accepted current-trial entry is unchanged until native acceptance.',
  );
}
