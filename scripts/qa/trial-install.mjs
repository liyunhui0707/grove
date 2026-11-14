// One interactive, generated-data-only installation/replacement acceptance session.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync, spawn } from 'node:child_process';
import { createInterface } from 'node:readline/promises';
import { fileURLToPath } from 'node:url';

const input = path.dirname(fileURLToPath(import.meta.url)),
  root = path.dirname(input);
const sha = (b) => crypto.createHash('sha256').update(b).digest('hex');
const run = (cmd, args, extra = {}) =>
  execFileSync(cmd, args, { encoding: 'utf8', timeout: 60000, ...extra });
const mode = process.argv[2];
if (
  !['prepare', 'start', 'verify', 'inspect'].includes(mode) ||
  process.platform !== 'darwin' ||
  process.arch !== 'arm64' ||
  !/^moment-grove-native-trial-\d{8}-[a-f0-9]{8}$/.test(path.basename(root))
)
  throw Error('Wrong trial scope');
const meta = fs.lstatSync(root);
if (meta.isSymbolicLink() || meta.uid !== process.getuid() || (meta.mode & 511) !== 448)
  throw Error('Unsafe trial root');
const raw = fs.readFileSync(input + '/manifest.json');
if (sha(raw) !== fs.readFileSync(root + '/expected-manifest.sha256', 'utf8').trim())
  throw Error('Independent manifest mismatch');
const manifest = JSON.parse(raw);
for (const f of manifest.files) {
  const p = path.resolve(input, f.path);
  if (
    !p.startsWith(input + '/') ||
    !fs.lstatSync(p).isFile() ||
    fs.lstatSync(p).isSymbolicLink() ||
    sha(fs.readFileSync(p)) !== f.sha256
  )
    throw Error('Trial input changed');
}
const data = root + '/runtime',
  sources = root + '/sources',
  target = root + '/exports-parent';
const installed = root + '/Applications/Moment Grove.app';
const candidate = root + '/candidate/Moment Grove.app';
const previous = root + '/previous/Moment Grove.app';
const binary = (app) => app + '/Contents/MacOS/moment-grove';
function write(name, value) {
  fs.writeFileSync(root + '/' + name, JSON.stringify(value, null, 2), { mode: 0o600 });
}
function bundle(app, expectedBinary, allFiles = false) {
  fs.accessSync(binary(app), fs.constants.X_OK);
  if (sha(fs.readFileSync(binary(app))) !== expectedBinary)
    throw Error('Wrong installed executable');
  run('/usr/bin/codesign', ['--verify', '--deep', '--strict', app]);
  const info = JSON.parse(
    run('/usr/bin/plutil', ['-convert', 'json', '-o', '-', app + '/Contents/Info.plist']),
  );
  if (
    info.CFBundleIdentifier !== 'app.momentgrove.desktop' ||
    info.CFBundleShortVersionString !== '0.1.0' ||
    run('/usr/bin/lipo', ['-archs', binary(app)]).trim() !== 'arm64'
  )
    throw Error('Wrong bundle metadata');
  if (allFiles)
    for (const f of manifest.appFiles) {
      const p = path.resolve(app, f.path);
      if (!p.startsWith(app + '/') || sha(fs.readFileSync(p)) !== f.sha256)
        throw Error('Installed resource mismatch');
    }
  return info;
}
function catalog() {
  const db = data + '/catalog.sqlite';
  // A closed app may leave committed transactions in WAL. SQLite must read
  // the complete database/WAL together; immutable=1 can hide those transactions.
  if (pids().length) throw Error('App still running; quit before catalog observation');
  return JSON.parse(
    run('/usr/bin/sqlite3', [
      '-readonly',
      '-json',
      'file:' + encodeURI(db) + '?mode=ro',
      'SELECT a.id,a.path,a.identity,a.rating,a.revision,p.revision AS pending FROM assets a LEFT JOIN pending_xmp p ON a.id=p.id ORDER BY a.id;',
    ]),
  );
}
function originals() {
  const fixture = JSON.parse(fs.readFileSync(root + '/fixtures.json'));
  const places = new Map(fixture.files.map((f) => [f.name, new Set([sources + '/' + f.name])]));
  const logs = data + '/moves';
  if (fs.existsSync(logs))
    for (const id of fs.readdirSync(logs)) {
      if (!/^[a-f0-9-]{36}$/.test(id)) continue;
      const p = logs + '/' + id + '/plan.json';
      if (!fs.existsSync(p)) continue;
      const job = JSON.parse(fs.readFileSync(p));
      if (
        job.root !== sources ||
        job.parent !== target ||
        !/^Moment-Grove-(Move|Quarantine)-\d+-[a-f0-9]{8}$/.test(job.directory)
      )
        throw Error('Move record outside trial scope');
      for (const item of job.items)
        for (const r of item.resources)
          if (places.has(r.name)) {
            if (!/^\.moment-grove-move-[a-f0-9-]+-\d+-[01]$/.test(r.stage))
              throw Error('Invalid staging name');
            places.get(r.name).add(sources + '/' + r.stage);
            places.get(r.name).add(target + '/' + job.directory + '/' + r.name);
          }
    }
  for (const f of fixture.files) {
    const found = [...places.get(f.name)].filter((p) => fs.existsSync(p));
    if (
      found.length !== 1 ||
      !fs.lstatSync(found[0]).isFile() ||
      fs.lstatSync(found[0]).isSymbolicLink() ||
      sha(fs.readFileSync(found[0])) !== f.sha256
    )
      throw Error('Generated original missing or changed');
  }
  return { verified: fixture.files.length, uniqueOriginals: true };
}
function snapshots() {
  return fs
    .readdirSync(data + '/backups')
    .filter((n) => /^[a-f0-9-]{36}\.sqlite3$/.test(n))
    .sort()
    .map((name) => {
      const file = data + '/backups/' + name,
        meta = fs.lstatSync(file);
      if (!meta.isFile() || meta.isSymbolicLink()) throw Error('Invalid trial snapshot');
      const lines = run('/usr/bin/sqlite3', [
        '-readonly',
        'file:' + encodeURI(file) + '?mode=ro&immutable=1',
        'PRAGMA integrity_check; PRAGMA application_id; PRAGMA user_version; SELECT count(*) FROM assets;',
      ])
        .trim()
        .split('\n');
      if (JSON.stringify(lines) !== JSON.stringify(['ok', String(0x4d475256), '2', '8']))
        throw Error('Snapshot integrity or schema mismatch');
      return { name, sha256: sha(fs.readFileSync(file)), integrity: 'ok', schema: 2, photos: 8 };
    });
}
function verifyOutputs() {
  const fixture = JSON.parse(fs.readFileSync(root + '/fixtures.json'));
  const expected = new Map(
    fixture.files.filter((f) => /^0[34]-/.test(f.name)).map((f) => [f.name, f]),
  );
  const directories = fs
    .readdirSync(target)
    .filter((n) => /^Moment-Grove-(?:Watermark-)?\d+-[a-f0-9]{8}$/.test(n));
  if (directories.length !== 2 || directories.filter((n) => n.includes('Watermark')).length !== 1)
    throw Error('Expected one two-photo copy and one watermark export');
  const verified = directories.sort().map((name) => {
    const dir = target + '/' + name;
    if (!fs.lstatSync(dir).isDirectory() || fs.lstatSync(dir).isSymbolicLink())
      throw Error('Invalid output directory');
    const portable = JSON.parse(fs.readFileSync(dir + '/moment-grove-export.json'));
    const watermark = name.includes('Watermark');
    if (
      portable.app !== 'Moment Grove' ||
      portable.format !== 1 ||
      portable.files.length !== 2 ||
      fs.readdirSync(dir).length !== 5 ||
      new Set(portable.files.map((f) => f.sourceRelative)).size !== 2
    )
      throw Error('Invalid portable output manifest');
    for (const [n, file] of portable.files.entries()) {
      if (
        !expected.has(file.sourceRelative) ||
        path.basename(file.name) !== file.name ||
        !file.name.endsWith('.jpg') ||
        file.rating !== (file.sourceRelative.startsWith('03') ? 4 : 5)
      )
        throw Error('Wrong output selection or rating');
      const p = dir + '/' + file.name,
        sidecar = p + '.xmp';
      for (const q of [p, sidecar])
        if (!fs.lstatSync(q).isFile() || fs.lstatSync(q).isSymbolicLink())
          throw Error('Invalid output file');
      const bytes = fs.readFileSync(p),
        xml = fs.readFileSync(sidecar);
      if (
        sha(bytes) !== file.sha256 ||
        bytes.length !== file.bytes ||
        sha(xml) !== file.sidecarSha256 ||
        !new RegExp('xmp:Rating="' + file.rating + '"').test(xml.toString())
      )
        throw Error('Output hash or XMP rating mismatch');
      const source = expected.get(file.sourceRelative);
      if (!watermark) {
        if (sha(bytes) !== source.sha256) throw Error('Copy changed original bytes');
      } else {
        const info = portable.rendered?.[n],
          text = file.sourceRelative.startsWith('03') ? '2026-10-27 温州' : '2026-11-10 温州';
        if (
          sha(bytes) === source.sha256 ||
          portable.watermark.city !== '温州' ||
          info?.source.text !== text ||
          !info.bounds ||
          info.outputWidth !== 3024 ||
          info.outputHeight !== 4032
        )
          throw Error('Watermark derivative information mismatch');
        const size = run('/usr/bin/sips', ['-g', 'pixelWidth', '-g', 'pixelHeight', p]);
        if (!/pixelWidth: 3024/.test(size) || !/pixelHeight: 4032/.test(size))
          throw Error('Watermark JPEG dimensions mismatch');
      }
    }
    return {
      name,
      kind: watermark ? 'watermark' : 'copy',
      verified: 2,
      manifestSha256: sha(fs.readFileSync(dir + '/moment-grove-export.json')),
    };
  });
  const rows = catalog();
  const expectedRatings = [0, 1, 4, 5, null, null, 3, null];
  for (const [n, rating] of expectedRatings.entries()) {
    const name = `0${n + 1}-generated.jpg`,
      row = rows.find((r) => r.path === sources + '/' + name);
    if (!row || row.rating !== rating || row.pending !== null)
      throw Error('Final rating or sync mismatch');
    const p = sources + '/' + name + '.xmp';
    if (
      rating !== null &&
      (!fs.existsSync(p) ||
        !new RegExp('xmp:Rating="' + (rating === 0 ? -1 : rating) + '"').test(
          fs.readFileSync(p, 'utf8'),
        ))
    )
      throw Error('Source XMP rating mismatch');
  }
  return { status: 'PASS', exports: verified, snapshots: snapshots(), sourceRatingsAndSync: true };
}
function observe(name) {
  const value = {
    observed: new Date().toISOString(),
    catalog: catalog(),
    originals: originals(),
    snapshots: snapshots(),
  };
  if (value.catalog.length !== 8 || new Set(value.catalog.map((r) => r.id)).size !== 8)
    throw Error('Wrong catalog occurrences');
  const seed = JSON.parse(fs.readFileSync(root + '/trial-seed.json'));
  if (
    !seed.baseline.every((b) =>
      value.catalog.some((r) => r.id === b.id && r.identity === b.identity),
    )
  )
    throw Error('Photo IDs or file identities changed');
  write(name + '.json', value);
  return value;
}
function pids() {
  const slash = String.fromCodePoint(92);
  const escaped = [...binary(installed)]
    .map((c) => ('.^$*+?()[]{}|'.includes(c) || c === slash ? slash + c : c))
    .join('');
  if (
    !new RegExp('^' + escaped + '$').test(binary(installed)) ||
    new RegExp('^' + escaped + '$').test(binary(installed).replace('.app', 'Xapp'))
  )
    throw Error('Invalid process path pattern');
  try {
    return run('/usr/bin/pgrep', ['-f', '^' + escaped + '( |$)'])
      .trim()
      .split('\n')
      .filter(Boolean)
      .map(Number);
  } catch (e) {
    if (e.status === 1) return [];
    throw e;
  }
}
if (mode === 'prepare') {
  const memoryProbe = JSON.parse(
    run(input + '/app-memory', [String(process.pid), process.execPath, '--single']),
  );
  if (!memoryProbe.mainSeen || memoryProbe.unreadableMembers || !memoryProbe.footprintBytes)
    throw Error('Coalition memory sampler unavailable on this OS');
  write('memory-sampler-preflight.json', {
    ...memoryProbe,
    scope:
      'CLI metric preflight only; App coalition is checked after LaunchServices starts the actual App',
  });
  const preparation = JSON.parse(run(process.execPath, [input + '/native-session.mjs', 'prepare']));
  fs.mkdirSync(target, { mode: 0o700 });
  fs.mkdirSync(root + '/tmp', { mode: 0o700 });
  const fixture = fs.readFileSync(root + '/fixtures.json');
  const seed = run(
    input + '/backend-tests',
    [
      '--exact',
      'benchmarks::trial::prepare_local_trial_fixture',
      '--ignored',
      '--nocapture',
      '--test-threads=1',
    ],
    {
      env: {
        ...process.env,
        TMPDIR: root + '/tmp/',
        MG_LOCAL_TRIAL_ROOT: root,
        MG_LOCAL_TRIAL_FIXTURES_SHA256: sha(fixture),
      },
    },
  );
  fs.writeFileSync(root + '/seed.stdout.log', seed, { mode: 0o600 });
  if (!/test result: ok\. 1 passed; 0 failed; 0 ignored;/.test(seed))
    throw Error('Fixture seeding failed');
  fs.mkdirSync(root + '/mount', { mode: 0o700 });
  fs.mkdirSync(root + '/candidate', { mode: 0o700 });
  let mounted = false;
  try {
    run('/usr/bin/hdiutil', [
      'attach',
      '-readonly',
      '-nobrowse',
      '-mountpoint',
      root + '/mount',
      input + '/' + manifest.dmg,
    ]);
    mounted = true;
    run('/usr/bin/ditto', [root + '/mount/Moment Grove.app', candidate]);
    bundle(candidate, manifest.binarySha256, true);
  } finally {
    if (mounted) run('/usr/bin/hdiutil', ['detach', root + '/mount']);
  }
  fs.mkdirSync(root + '/zip-check', { mode: 0o700 });
  run('/usr/bin/ditto', ['-x', '-k', input + '/' + manifest.zip, root + '/zip-check']);
  bundle(root + '/zip-check/Moment Grove/Moment Grove.app', manifest.binarySha256, true);
  fs.mkdirSync(root + '/previous', { mode: 0o700 });
  run('/usr/bin/ditto', ['-x', '-k', input + '/previous-app.zip', root + '/previous']);
  bundle(previous, manifest.previousBinarySha256);
  fs.mkdirSync(root + '/Applications', { mode: 0o700 });
  fs.cpSync(previous, installed, { recursive: true, errorOnExist: true });
  if (pids().length) throw Error('Trial app unexpectedly running during preparation');
  const ready = {
    status: 'INSTALLED_NOT_LAUNCHED',
    preparation,
    installedFromDmgAndZipMatch: true,
    candidateBundle: bundle(candidate, manifest.binarySha256, true),
    previousBinarySha256: manifest.previousBinarySha256,
    binarySha256: manifest.binarySha256,
    sourceDigest: manifest.sourceDigest,
    nativeGui: 'NOT_RUN',
    quarantineAttributes: 'SSH transfer; not a browser-download Gatekeeper test',
    observed: observe('seed-observation'),
  };
  write('trial-ready.json', ready);
  console.log(JSON.stringify(ready));
} else if (mode === 'inspect') {
  console.log(JSON.stringify(observe('inspection-observation')));
} else if (mode === 'verify') {
  if (pids().length) throw Error('Quit trial app before final observation');
  console.log(
    JSON.stringify({
      ...observe('final-observation'),
      installed: bundle(installed, manifest.binarySha256, true),
      outputs: fs.existsSync(root + '/trial-session-complete.json') ? verifyOutputs() : 'NOT_RUN',
    }),
  );
} else {
  if (!process.stdin.isTTY) throw Error('请从Air桌面终端双击Start Trial QA.command启动。');
  if (pids().length) throw Error('本批应用仍在运行，请用⌘Q退出后再启动。');
  if (fs.existsSync(root + '/session-started.json'))
    throw Error('已有启动记录；为保留现场已停止，请反馈终端错误，由开发会话检查后恢复入口。');
  write('session-started.json', { started: new Date().toISOString(), pid: process.pid });
  const terminal = createInterface({ input: process.stdin, output: process.stdout });
  const resumePrevious = fs.existsSync(root + '/resume-after-previous.json')
    ? JSON.parse(fs.readFileSync(root + '/resume-after-previous.json'))
    : null;
  const sessions = resumePrevious ? JSON.parse(fs.readFileSync(root + '/sessions.json')) : [];
  if (
    resumePrevious &&
    (sha(fs.readFileSync(root + '/sessions.json')) !== resumePrevious.sessionsSha256 ||
      JSON.stringify(catalog()) !== JSON.stringify(resumePrevious.catalog))
  )
    throw Error('Retained first-stage evidence changed');
  async function launch(label, instructions, expected) {
    bundle(installed, expected, expected === manifest.binarySha256);
    console.log(
      '\n' +
        instructions +
        '\n资料来源：' +
        sources +
        '\n目标父目录：' +
        target +
        '\n本段结束请用⌘Q退出应用，终端会继续。\n',
    );
    await terminal.question('解锁Air并读完本段操作后，按回车打开应用：');
    const log = root + '/logs/' + label;
    const started = Date.now(),
      samples = [],
      errors = [];
    const child = spawn(
      '/usr/bin/open',
      [
        '-n',
        '-W',
        '--env',
        'MOMENT_GROVE_DATA_DIR=' + data,
        '--stdout',
        log + '.stdout.log',
        '--stderr',
        log + '.stderr.log',
        installed,
      ],
      { stdio: ['ignore', 'pipe', 'pipe'] },
    );
    let launchOutput = '';
    for (const stream of [child.stdout, child.stderr])
      stream.on('data', (chunk) => {
        launchOutput += chunk;
        process.stdout.write(chunk);
        fs.appendFileSync(log + '.launch.log', chunk, { mode: 0o600 });
      });
    let pid = null,
      memoryLimit = false,
      watchdogTriggered = false;
    const collect = () => {
      try {
        const matches = pids();
        if (!matches.length) return;
        if (matches.length !== 1) throw Error('Ambiguous app process');
        pid = matches[0];
        const sample = JSON.parse(
          run(input + '/app-memory', [String(pid), binary(installed)], { timeout: 5000 }),
        );
        samples.push({ seconds: (Date.now() - started) / 1000, ...sample });
        if (
          !sample.mainSeen ||
          sample.unreadableMembers ||
          sample.members < 1 ||
          !Number.isFinite(sample.footprintBytes) ||
          sample.footprintBytes <= 0
        )
          throw Error('Incomplete app memory observation');
        if (sample.footprintBytes > 1536 * 1024 * 1024) {
          memoryLimit = true;
          process.kill(pid, 'SIGTERM');
        }
        write('memory-' + label + '.json', samples);
      } catch (e) {
        if (pids().length) errors.push(String(e));
      }
    };
    const timer = setInterval(collect, 1000);
    const watchdog = setTimeout(
      () => {
        watchdogTriggered = true;
        if (pid && pids().includes(pid)) process.kill(pid, 'SIGTERM');
        child.kill('SIGTERM');
      },
      25 * 60 * 1000,
    );
    const code = await new Promise((resolve, reject) => {
      child.once('error', reject);
      child.once('close', (code) => resolve(code));
    });
    clearInterval(timer);
    clearTimeout(watchdog);
    const result = {
      label,
      launchServicesCode: code,
      launchOutput,
      appExitCode: 'not available through LaunchServices',
      appPid: pid,
      appAbsent: !pids().length,
      memoryLimit,
      watchdogTriggered,
      memorySamples: samples.length,
      maxCoalitionFootprintMiB: samples.length
        ? Math.max(...samples.map((s) => s.footprintBytes / 1048576))
        : null,
      memoryErrors: errors,
      maxCoalitionRssMiB: samples.length
        ? Math.max(...samples.map((s) => s.rssBytes / 1048576))
        : null,
    };
    sessions.push(result);
    write('sessions.json', sessions);
    if (
      code !== 0 ||
      !result.appAbsent ||
      memoryLimit ||
      watchdogTriggered ||
      !samples.length ||
      errors.length ||
      result.maxCoalitionFootprintMiB > 1024
    )
      throw Error('Launch, exit or coalition memory conditions failed; keep this scene');
  }
  try {
    if (resumePrevious) console.log('第一段已完成并核验，直接接着第二段；无需重做07评分。');
    else
      await launch(
        'previous',
        '第一段：旧候选。打开本批sources，应有8张，01=0、02=1、03=4、04=5。给07评3分，然后⌘Q。',
        manifest.previousBinarySha256,
      );
    const old = observe('previous-observation');
    if (
      !old.catalog.some((r) => r.path.endsWith('/07-generated.jpg') && r.rating === 3) ||
      old.catalog.filter((r) => r.pending !== null).length !== 5
    )
      throw Error('Expected old-candidate rating and pending state');
    fs.mkdirSync(root + '/saved-previous', { mode: 0o700 });
    fs.renameSync(installed, root + '/saved-previous/Moment Grove.app');
    fs.cpSync(candidate, installed, { recursive: true, errorOnExist: true });
    await launch(
      'candidate',
      '第二段：新候选。\n1. 打开sources，确认8张及07=3；左下“使用说明与资料”应显示0.1.0和独立测试目录。\n2. 比较03/04，试评分并撤销；最终03=4、04=5、07=3。\n3. 评分同步本页5项，应待同步0。\n4. “家庭精选”应2张，导出副本到本批exports-parent，完成2/2。再用“水印”填温州、备用日期2026-11-10，同一父目录导出2/2。\n5. 回所有照片，安全整理选低分隔离0–1，只含01/02；核对并确认2/2后⌘Q。',
      manifest.binarySha256,
    );
    const moved = observe('candidate-before-restart');
    if (
      moved.catalog.filter((r) => !r.path.startsWith(sources + '/')).length !== 2 ||
      moved.catalog.some((r) => r.pending !== null)
    )
      throw Error('Expected two isolated photos and synced ratings');
    await launch(
      'candidate-restart',
      '第三段：重启新候选。直接打开安全整理，从最近记录“核对撤回”，重新选择本批sources和exports-parent（若要求），勾选确认撤回2/2。清除筛选/搜索确认sources共8张，07仍3分；创建资料库备份，再⌘Q。',
      manifest.binarySha256,
    );
    const restored = observe('candidate-restored');
    write('native-output-verification.json', verifyOutputs());
    if (restored.snapshots.length <= moved.snapshots.length)
      throw Error('Expected a new manual catalog snapshot after restart');
    if (restored.catalog.some((r) => !r.path.startsWith(sources + '/') || r.pending !== null))
      throw Error('Expected all photos restored and no pending sync');
    fs.mkdirSync(root + '/saved-candidate', { mode: 0o700 });
    fs.renameSync(installed, root + '/saved-candidate/Moment Grove.app');
    fs.renameSync(root + '/saved-previous/Moment Grove.app', installed);
    await launch(
      'rollback',
      '第四段：回退旧候选。只打开sources核对8张、01=0、02=1、03=4、04=5、07=3，以及待同步0。不要再评分或移动，确认后⌘Q。',
      manifest.previousBinarySha256,
    );
    const rollback = observe('rollback-observation');
    if (JSON.stringify(rollback.catalog) !== JSON.stringify(restored.catalog))
      throw Error('Rollback changed catalog bindings, ratings or pending state');
    fs.mkdirSync(root + '/rollback-observed', { mode: 0o700 });
    fs.renameSync(installed, root + '/rollback-observed/Moment Grove.app');
    fs.renameSync(root + '/saved-candidate/Moment Grove.app', installed);
    write('trial-session-complete.json', {
      status: 'MANUAL_FEEDBACK_REQUIRED',
      sessions,
      installed: bundle(installed, manifest.binarySha256, true),
      final: observe('session-final'),
      finished: new Date().toISOString(),
    });
    console.log(
      '\n本次集中操作已记录，当前安装位置已恢复新候选。请一次反馈四段观感/操作是否正常；自动文件核验由开发会话收尾。',
    );
  } finally {
    terminal.close();
  }
}
