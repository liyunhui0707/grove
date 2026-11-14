import { createServer } from 'node:http';
import { createHash } from 'node:crypto';
import { spawn, execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, mkdirSync, lstatSync, readdirSync, cpSync } from 'node:fs';
import { dirname, join, resolve, sep, extname } from 'node:path';
import { fileURLToPath } from 'node:url';

const input = dirname(fileURLToPath(import.meta.url)),
  root = dirname(input),
  results = join(root, 'results'),
  tmp = join(root, 'tmp');
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
const raw = readFileSync(join(input, 'manifest.json'));
if (!/^[a-f0-9]{64}$/.test(process.argv[2] ?? '') || sha(raw) !== process.argv[2])
  throw new Error('Manifest mismatch');
const manifest = JSON.parse(raw);
if (process.platform !== 'darwin' || process.arch !== 'arm64')
  throw new Error('macOS arm64 required');
for (const f of manifest.files) {
  const path = resolve(input, f.path);
  if (
    !path.startsWith(input + sep) ||
    !lstatSync(path).isFile() ||
    lstatSync(path).isSymbolicLink() ||
    sha(readFileSync(path)) !== f.sha256
  )
    throw new Error('QA input mismatch');
}
mkdirSync(results, { mode: 0o700 });
mkdirSync(tmp, { mode: 0o700 });
const chrome = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const report = {
  status: 'INCOMPLETE',
  started: new Date().toISOString(),
  node: process.version,
  os: execFileSync('/usr/bin/sw_vers', ['-productVersion'], {
    encoding: 'utf8',
    timeout: 5000,
  }).trim(),
  candidate: manifest,
  scope:
    'Production browser preview in existing headless Chrome; no native Tauri or OS-picker acceptance',
};
const staticFiles = new Map(
  manifest.files
    .filter((f) => f.path.startsWith('dist/'))
    .map((f) => ['/' + f.path.slice(5), join(input, f.path)]),
);
const server = createServer((req, res) => {
  const path = new URL(req.url, 'http://127.0.0.1').pathname;
  const file = staticFiles.get(path === '/' ? '/index.html' : path);
  if (!file || !['GET', 'HEAD'].includes(req.method)) {
    res.writeHead(404);
    res.end();
    return;
  }
  res.setHeader(
    'Content-Type',
    { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' }[extname(file)] ??
      'application/octet-stream',
  );
  res.setHeader('Cache-Control', 'no-store');
  res.end(req.method === 'HEAD' ? '' : readFileSync(file));
});
try {
  await new Promise((ok, fail) => {
    server.once('error', fail);
    server.listen(0, '127.0.0.1', ok);
  });
  const baseURL = `http://127.0.0.1:${server.address().port}`;
  const args = [
    join(input, 'node_modules/@playwright/test/cli.js'),
    'test',
    '--config',
    join(input, 'browser.config.mjs'),
  ];
  const child = spawn(process.execPath, args, {
    cwd: input,
    detached: true,
    env: {
      ...process.env,
      PATH: `${dirname(process.execPath)}:${process.env.PATH ?? ''}`,
      TMPDIR: `${tmp}/`,
      MG_QA_URL: baseURL,
      MG_QA_CHROME: chrome,
      PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD: '1',
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let stdout = '',
    stderr = '',
    watchdog = false,
    stopping = false,
    hardTimer;
  const stop = () => {
    if (stopping || !child.pid) return;
    stopping = true;
    try {
      process.kill(-child.pid, 'SIGTERM');
    } catch {}
    hardTimer = setTimeout(() => {
      try {
        process.kill(-child.pid, 'SIGKILL');
      } catch {}
    }, 2000);
  };
  const timer = setTimeout(() => {
    watchdog = true;
    stop();
  }, 360000);
  child.stdout.on('data', (c) => {
    stdout += c;
    if (stdout.length > 10_000_000) stop();
  });
  child.stderr.on('data', (c) => {
    stderr += c;
    if (stderr.length > 10_000_000) stop();
  });
  let spawnError = null;
  child.on('error', (e) => {
    spawnError = e.message;
  });
  const outcome = await new Promise((ok) =>
    child.on('close', (code, signal) => ok({ code, signal })),
  );
  clearTimeout(timer);
  if (hardTimer) clearTimeout(hardTimer);
  let residualGroup = false;
  if (child.pid) {
    try {
      process.kill(-child.pid, 0);
      residualGroup = true;
    } catch (e) {
      if (e.code !== 'ESRCH') residualGroup = 'unknown';
    }
  }
  writeFileSync(join(results, 'stdout.log'), stdout, { mode: 0o600 });
  writeFileSync(join(results, 'stderr.log'), stderr, { mode: 0o600 });
  Object.assign(report, {
    command: [process.execPath, ...args],
    cwd: input,
    ...outcome,
    watchdog,
    spawnError,
    residualGroup,
  });
  if (outcome.code !== 0 || outcome.signal !== null || watchdog || spawnError || residualGroup)
    throw new Error('Browser process conditions failed');
  const data = JSON.parse(readFileSync(join(results, 'playwright.json'), 'utf8'));
  report.stats = data.stats;
  if (
    data.stats.expected !== (manifest.expectedTests ?? 9) ||
    data.stats.unexpected !== 0 ||
    data.stats.skipped !== 0 ||
    data.stats.flaky !== 0
  )
    throw new Error('Browser passing-count contract mismatch');
  cpSync(join(input, 'test-results'), join(results, 'screenshots'), {
    recursive: true,
    errorOnExist: true,
  });
  report.status = 'PASS';
} catch (e) {
  report.status = 'FAIL';
  report.error = String(e.message ?? e);
}
server.closeAllConnections();
await new Promise((ok) => server.close(ok));
report.serverClosed = !server.listening;
report.finished = new Date().toISOString();
writeFileSync(join(results, 'report.json'), JSON.stringify(report, null, 2), { mode: 0o600 });
const outputs = [];
function visit(dir, prefix = '') {
  for (const name of readdirSync(dir).sort()) {
    const path = join(dir, name),
      relative = prefix + name,
      s = lstatSync(path);
    if (s.isDirectory()) visit(path, `${relative}/`);
    else if (s.isFile())
      outputs.push({ path: relative, bytes: s.size, sha256: sha(readFileSync(path)) });
    else throw new Error('Unexpected output object');
  }
}
visit(results);
const outputManifest = JSON.stringify(outputs, null, 2);
writeFileSync(join(results, 'files.json'), outputManifest, { mode: 0o600 });
console.log(
  JSON.stringify({
    status: report.status,
    stats: report.stats,
    error: report.error,
    outputManifestSha256: sha(outputManifest),
  }),
);
process.exitCode = report.status === 'PASS' ? 0 : 1;
