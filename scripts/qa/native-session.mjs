// Private, user-operated native QA session. Never grants permissions or drives other apps.
import { batchLayout } from './candidate-layout.mjs';
import { spawn, execFileSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { deflateSync } from 'node:zlib';
import { createInterface } from 'node:readline/promises';
import {
  readFileSync,
  writeFileSync,
  mkdirSync,
  lstatSync,
  existsSync,
  renameSync,
  readdirSync,
  openSync,
  closeSync,
} from 'node:fs';
import { dirname, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const input = dirname(fileURLToPath(import.meta.url)),
  root = dirname(input);
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
if (
  process.platform !== 'darwin' ||
  process.arch !== 'arm64' ||
  !/^(?:moment-grove|grove)-native-/.test(root.split(sep).at(-1))
)
  throw new Error('Run only from a prepared macOS arm64 QA batch');
const directory = lstatSync(root);
if (
  !directory.isDirectory() ||
  directory.isSymbolicLink() ||
  directory.uid !== process.getuid() ||
  (directory.mode & 0o777) !== 0o700
)
  throw new Error('Unsafe private batch directory');
const expected = readFileSync(join(root, 'expected-manifest.sha256'), 'utf8').trim();
const raw = readFileSync(join(input, 'manifest.json'));
if (!/^[a-f0-9]{64}$/.test(expected) || sha(raw) !== expected)
  throw new Error('Independent input manifest mismatch');
const manifest = JSON.parse(raw);
const layout = batchLayout(manifest);
for (const f of manifest.files) {
  const path = resolve(input, f.path),
    m = lstatSync(path);
  if (
    !path.startsWith(input + sep) ||
    !m.isFile() ||
    m.isSymbolicLink() ||
    sha(readFileSync(path)) !== f.sha256
  )
    throw new Error('Input integrity check failed');
}
const sources = join(root, 'sources'),
  moved = join(root, 'moved-sources'),
  data = join(root, 'runtime'),
  logs = join(root, 'logs');
const fixtureFile = join(root, 'fixtures.json');
function verify() {
  if (existsSync(sources) === existsSync(moved))
    throw new Error('Expected exactly one generated source directory');
  const dir = existsSync(sources) ? sources : moved;
  const meta = lstatSync(dir);
  if (!meta.isDirectory() || meta.isSymbolicLink())
    throw new Error('Generated source directory was replaced');
  const fixtures = JSON.parse(readFileSync(fixtureFile, 'utf8'));
  if (fixtures.batch !== manifest.batch || fixtures.files.length !== 8)
    throw new Error('Wrong fixture set');
  let distributed = 0;
  for (const f of [...fixtures.files, ...(fixtures.sidecars ?? [])]) {
    if (!/^0[1-8]-generated\.jpg(?:\.xmp)?$/.test(f.name))
      throw new Error('Unexpected fixture filename');
    const candidates = new Set([join(dir, f.name)]);
    if (manifest.fixtureProfile === 'moves') {
      const journals = join(data, 'moves');
      if (existsSync(journals))
        for (const id of readdirSync(journals)) {
          if (!/^[a-f0-9-]{36}$/.test(id)) continue;
          const record = join(journals, id, 'plan.json');
          if (!existsSync(record)) continue;
          const info = lstatSync(record);
          if (!info.isFile() || info.isSymbolicLink() || info.size > 1048576)
            throw new Error('Invalid move QA record');
          const plan = JSON.parse(readFileSync(record, 'utf8'));
          if (
            plan.id !== id ||
            plan.root !== sources ||
            plan.parent !== join(root, 'exports-parent') ||
            !/^(?:Grove|Moment-Grove)-(Move|Quarantine)-\d+-[a-f0-9]{8}$/.test(plan.directory)
          )
            throw new Error('Move outside this QA batch');
          for (const item of plan.items)
            for (const resource of item.resources)
              if (resource.name === f.name) {
                if (
                  dirname(item.path) !== sources ||
                  !/^\.moment-grove-move-[a-f0-9-]+-\d+-[01]$/.test(resource.stage)
                )
                  throw new Error('Invalid move QA resource');
                candidates.add(join(sources, resource.stage));
                candidates.add(join(plan.parent, plan.directory, resource.name));
              }
        }
    }
    let found = 0;
    for (const candidate of candidates)
      if (existsSync(candidate)) {
        const m = lstatSync(candidate);
        if (!m.isFile() || m.isSymbolicLink() || sha(readFileSync(candidate)) !== f.sha256)
          throw new Error('Generated original changed');
        found++;
        if (candidate !== join(dir, f.name)) distributed++;
      }
    if (found !== 1)
      throw new Error('Expected one unchanged occurrence of each generated resource');
  }
  return {
    dir,
    originalHashesUnchanged: true,
    files: fixtures.files.length,
    sidecars: fixtures.sidecars?.length ?? 0,
    distributed,
  };
}
function crc32(buffer) {
  let c = 0xffffffff;
  for (const b of buffer) {
    c ^= b;
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (c & 1 ? 0xedb88320 : 0);
  }
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const body = Buffer.concat([Buffer.from(type), data]),
    out = Buffer.alloc(body.length + 8);
  out.writeUInt32BE(data.length, 0);
  body.copy(out, 4);
  out.writeUInt32BE(crc32(body), out.length - 4);
  return out;
}
function png(width, height, seed) {
  const pixels = Buffer.alloc((width * 3 + 1) * height);
  for (let y = 0; y < height; y++) {
    const row = y * (width * 3 + 1);
    pixels[row] = 0;
    for (let x = 0; x < width; x++) {
      const i = row + 1 + x * 3,
        n = (x * 13 + y * 7 + seed * 19) % 19;
      pixels[i] = (x / 25 + seed * 17 + n) % 256;
      pixels[i + 1] = (y / 17 + seed * 29 + n) % 256;
      pixels[i + 2] = ((x + y) / 30 + seed * 11 + n) % 256;
    }
  }
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8;
  header[9] = 2;
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk('IHDR', header),
    chunk('IDAT', deflateSync(pixels, { level: 1 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}
function active() {
  const path = join(root, 'current-session.json');
  if (!existsSync(path)) return false;
  const previous = JSON.parse(readFileSync(path, 'utf8'));
  if (previous.status !== 'RUNNING') return false;
  for (const pid of [previous.launcherPid, previous.appPid]) {
    if (!Number.isInteger(pid) || pid <= 0) continue;
    try {
      process.kill(pid, 0);
      return true;
    } catch (e) {
      if (e.code !== 'ESRCH') throw e;
    }
  }
  return false;
}
// Synthetic camera-local EXIF and GPS, only for the explicitly selected watermark QA profile.
function watermarkExif(jpeg, orientation, date) {
  const u16 = (n) => {
    const b = Buffer.alloc(2);
    b.writeUInt16LE(n);
    return b;
  };
  const u32 = (n) => {
    const b = Buffer.alloc(4);
    b.writeUInt32LE(n);
    return b;
  };
  const entry = (tag, type, count, offset) =>
    Buffer.concat([u16(tag), u16(type), u32(count), u32(offset)]);
  const firstEnd = 8 + 2 + (date ? 3 : 2) * 12 + 4;
  const gps = date ? Math.ceil((firstEnd + 57) / 2) * 2 : firstEnd;
  const parts = [
    Buffer.from([73, 73, 42, 0, 8, 0, 0, 0]),
    u16(date ? 3 : 2),
    entry(0x112, 3, 1, orientation),
  ];
  if (date) parts.push(entry(0x8769, 4, 1, firstEnd));
  parts.push(entry(0x8825, 4, 1, gps), u32(0));
  if (date)
    parts.push(
      u16(2),
      entry(0x9003, 2, 20, firstEnd + 30),
      entry(0x9011, 2, 7, firstEnd + 50),
      u32(0),
      Buffer.from(date + '\0+14:00\0'),
    );
  const before = Buffer.concat(parts);
  const tiff = Buffer.concat([
    before,
    Buffer.alloc(gps - before.length),
    u16(2),
    entry(1, 2, 2, 78),
    entry(2, 5, 3, gps + 30),
    u32(0),
    ...[28, 1, 0, 1, 0, 1].map(u32),
  ]);
  const body = Buffer.concat([Buffer.from('Exif\0\0'), tiff]);
  const marker = Buffer.from([255, 225, 0, 0]);
  marker.writeUInt16BE(body.length + 2, 2);
  return Buffer.concat([jpeg.subarray(0, 2), marker, body, jpeg.subarray(2)]);
}
async function launch() {
  verify();
  if (active()) throw new Error('A QA session may still be running; quit it first');
  const session = randomUUID(),
    started = Date.now(),
    reportPath = join(logs, `${session}.json`);
  const options = [layout.app, layout.product + ' QA.app'].map(
    (name) => name + '/Contents/MacOS/' + layout.binary,
  );
  const entries = manifest.files.filter((f) => options.includes(f.path));
  if (entries.length !== 1 || entries[0].sha256 !== manifest.binarySha256)
    throw Error('Expected one hash-verified QA executable');
  const executable = join(input, entries[0].path);
  const out = openSync(join(logs, `${session}.stdout.log`), 'wx', 0o600),
    err = openSync(join(logs, `${session}.stderr.log`), 'wx', 0o600);
  const child = spawn(executable, [], {
    cwd: input,
    env: { ...process.env, MOMENT_GROVE_DATA_DIR: data },
    stdio: ['ignore', out, err],
  });
  closeSync(out);
  closeSync(err);
  let spawnError = null,
    watchdog = false;
  const samples = [];
  child.on('error', (e) => {
    spawnError = e.message;
  });
  const report = {
    status: 'RUNNING',
    session,
    launcherPid: process.pid,
    appPid: child.pid,
    started: new Date(started).toISOString(),
    candidateManifestSha256: expected,
    media: verify(),
    note: 'User-operated GUI; RSS is main process only, not WebView/GPU or an acceptance verdict',
  };
  writeFileSync(join(root, 'current-session.json'), JSON.stringify(report, null, 2), {
    mode: 0o600,
  });
  const terminate = () => {
    child.kill('SIGTERM');
  };
  process.once('SIGINT', terminate);
  process.once('SIGTERM', terminate);
  const watchdogTimer = setTimeout(
    () => {
      watchdog = true;
      child.kill('SIGTERM');
      setTimeout(() => {
        if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
      }, 2000).unref();
    },
    35 * 60 * 1000,
  );
  const sampler = setInterval(() => {
    if (!child.pid) return;
    try {
      const rss = execFileSync('/bin/ps', ['-o', 'rss=', '-p', String(child.pid)], {
        encoding: 'utf8',
        timeout: 2000,
      }).trim();
      if (/^\d+$/.test(rss))
        samples.push({
          seconds: (Date.now() - started) / 1000,
          mainProcessRssMiB: Number(rss) / 1024,
        });
      writeFileSync(join(logs, `${session}.rss.json`), JSON.stringify(samples, null, 2), {
        mode: 0o600,
      });
    } catch {
      /* A quitting app can disappear before ps. */
    }
  }, 5000);
  console.log(
    `Starting the isolated QA candidate. Test photos: ${verify().dir}\nConfirm the window appears, then quit with Cmd+Q when finished. This session is bounded to 35 minutes.`,
  );
  const outcome = await new Promise((ok) =>
    child.on('close', (code, signal) => ok({ code, signal })),
  );
  clearInterval(sampler);
  clearTimeout(watchdogTimer);
  process.removeListener('SIGINT', terminate);
  process.removeListener('SIGTERM', terminate);
  Object.assign(report, {
    status: 'MANUAL_RESULTS_REQUIRED',
    finished: new Date().toISOString(),
    ...outcome,
    spawnError,
    watchdog,
    mainProcessSamples: samples.length,
    media: verify(),
  });
  writeFileSync(reportPath, JSON.stringify(report, null, 2), { mode: 0o600 });
  writeFileSync(join(root, 'current-session.json'), JSON.stringify(report, null, 2), {
    mode: 0o600,
  });
  console.log(`Session saved: ${reportPath}\nGUI acceptance remains pending your observations.`);
  process.exitCode = outcome.code === 0 && !spawnError && !watchdog ? 0 : 1;
}
const mode = process.argv[2];
if (mode === 'prepare') {
  for (const dir of [sources, data, logs, join(root, 'staging')]) mkdirSync(dir, { mode: 0o700 });
  const files = [],
    sidecars = [];
  for (let i = 1; i <= 8; i++) {
    const width = i % 2 ? 4032 : 3024,
      height = i % 2 ? 3024 : 4032,
      name = `0${i}-generated.jpg`,
      stage = join(root, 'staging', `${i}.png`);
    writeFileSync(stage, png(width, height, i), { mode: 0o600 });
    execFileSync(
      '/usr/bin/sips',
      ['-s', 'format', 'jpeg', '-s', 'formatOptions', '88', stage, '--out', join(sources, name)],
      { timeout: 30000, stdio: 'ignore' },
    );
    const fixture = { name, width, height };
    if (manifest.fixtureProfile === 'watermark') {
      fixture.orientation = i === 3 ? 6 : 1;
      fixture.date =
        i === 4
          ? null
          : i === 8
            ? '2026:11:01 23:59:58'
            : `2026:10:${String(24 + i).padStart(2, '0')} 23:59:58`;
      writeFileSync(
        join(sources, name),
        watermarkExif(readFileSync(join(sources, name)), fixture.orientation, fixture.date),
      );
    }
    files.push({ ...fixture, sha256: sha(readFileSync(join(sources, name))) });
    if (manifest.fixtureProfile === 'moves' && i <= 2) {
      const name = `0${i}-generated.jpg.xmp`;
      const xml =
        '<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmlns:generated="urn:moment-grove:generated-qa" xmp:Rating="0"><generated:Note>Generated QA field to preserve</generated:Note></rdf:Description></rdf:RDF></x:xmpmeta>';
      writeFileSync(join(sources, name), xml, { mode: 0o600 });
      sidecars.push({ name, sha256: sha(Buffer.from(xml)) });
    }
  }
  writeFileSync(
    fixtureFile,
    JSON.stringify(
      {
        batch: manifest.batch,
        provenance: 'Original procedurally generated RGB test images; no personal media',
        files,
        sidecars,
      },
      null,
      2,
    ),
    { mode: 0o600 },
  );
  const result = {
    status: 'PREPARED_NOT_LAUNCHED',
    created: new Date().toISOString(),
    ...verify(),
    guiAcceptance: 'NOT_RUN',
    permissionsChanged: false,
  };
  writeFileSync(join(root, 'preparation.json'), JSON.stringify(result, null, 2), { mode: 0o600 });
  console.log(JSON.stringify(result));
} else if (mode === 'launch') {
  await launch();
} else if (mode === 'move') {
  verify();
  if (active() || !existsSync(sources) || existsSync(moved))
    throw new Error('Quit QA and use only the original generated directory');
  if (!process.stdin.isTTY) throw new Error('Move only from an interactive QA Terminal');
  console.log(
    `Move ONLY this batch's 8 generated photos and their test sidecars:\n${sources}\n→ ${moved}`,
  );
  const prompt = createInterface({ input: process.stdin, output: process.stdout });
  const answer = await prompt.question('Type MOVE to confirm: ');
  prompt.close();
  if (answer !== 'MOVE') throw new Error('Cancelled; no files moved');
  renameSync(sources, moved);
  console.log(JSON.stringify(verify()));
} else if (mode === 'verify') {
  console.log(JSON.stringify(verify()));
} else {
  throw new Error('Choose prepare, launch, move, or verify; no default GUI launch');
}
