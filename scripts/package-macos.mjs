// Local candidate packaging; publication is a separate, explicit operation.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { root } from './toolchain.mjs';

if (process.platform !== 'darwin' || process.arch !== 'arm64')
  throw Error('This packaging entry supports the macOS Apple Silicon candidate only');
const args = process.argv.slice(2);
if (args.some((a) => a !== '--prerelease')) throw Error('Unknown packaging option');
const channel = args.includes('--prerelease') ? 'public-prerelease' : 'local-trial';
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
if (pkg.version !== config.version || config.identifier !== 'app.momentgrove.desktop')
  throw Error('Candidate version or application identity mismatch');
const out = path.join(root, '.local', 'deliveries', `macos-${crypto.randomUUID()}`);
fs.mkdirSync(out, { recursive: true, mode: 0o700 });
process.stdout.write(
  run(process.execPath, [
    path.join(root, 'scripts/desktop.mjs'),
    'build',
    '--bundles',
    'app',
    '--no-sign',
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
// Ad-hoc signing binds the resource envelope; it is not Developer ID or notarization.
run('/usr/bin/codesign', ['--force', '--sign', '-', '--timestamp=none', app]);
run('/usr/bin/codesign', ['--verify', '--deep', '--strict', app]);
const info = JSON.parse(
  run('/usr/bin/plutil', ['-convert', 'json', '-o', '-', path.join(app, 'Contents/Info.plist')]),
);
const binary = path.join(app, 'Contents/MacOS/moment-grove');
if (
  info.CFBundleIdentifier !== config.identifier ||
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
const sourcePaths = [
  'package.json',
  'package-lock.json',
  'vite.config.ts',
  'tsconfig.json',
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
fs.writeFileSync(path.join(out, 'source-manifest.json'), sourceRaw, { mode: 0o600 });
for (const [from, to] of [
  ['docs/user-guide.md', '使用说明.md'],
  ['docs/format-support.md', 'format-support.md'],
  ['LICENSE', 'LICENSE'],
  ['docs/third-party.md', 'THIRD-PARTY.md'],
  ['THIRD-PARTY-NOTICES.txt', 'THIRD-PARTY-NOTICES.txt'],
])
  fs.copyFileSync(path.join(root, from), path.join(payload, to));
const candidate = {
  product: 'Moment Grove',
  version: pkg.version,
  channel,
  architecture: 'arm64',
  minimumSystemDeclaration: info.LSMinimumSystemVersion,
  testedSystems: [],
  signing: 'ad-hoc; no Developer ID or notarization',
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
const zip = path.join(out, `Moment-Grove-${pkg.version}-macOS-arm64.zip`);
run('/usr/bin/ditto', ['-c', '-k', '--sequesterRsrc', '--keepParent', payload, zip]);
const dmg = path.join(out, `Moment-Grove-${pkg.version}-macOS-arm64.dmg`);
run('/usr/bin/hdiutil', [
  'create',
  '-volname',
  channel === 'public-prerelease' ? 'Moment Grove 0.1.0 Preview' : 'Moment Grove Trial',
  '-srcfolder',
  payload,
  '-format',
  'UDZO',
  dmg,
]);
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
};
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
