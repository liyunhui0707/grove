// Run only from an independently hash-verified private QA bundle, never from a media library.
import { spawn, execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  readFileSync,
  writeFileSync,
  appendFileSync,
  mkdirSync,
  readdirSync,
  statSync,
} from 'node:fs';
import { dirname, join, basename } from 'node:path';
import { fileURLToPath } from 'node:url';

const input = dirname(fileURLToPath(import.meta.url));
const root = dirname(input),
  results = join(root, 'results'),
  tmp = join(root, 'tmp');
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
const bytes = readFileSync(join(input, 'manifest.json'));
if (!/^[a-f0-9]{64}$/.test(process.argv[2] ?? '') || sha(bytes) !== process.argv[2])
  throw new Error('Manifest hash mismatch');
const manifest = JSON.parse(bytes);
if (!['regression', 'baseline', 'soak', 'batch', 'watermark'].includes(manifest.mode))
  throw new Error('Unknown test mode');
if (process.platform !== 'darwin' || process.arch !== 'arm64')
  throw new Error('This batch requires macOS arm64');
for (const file of manifest.files) {
  if (
    basename(file.path) !== file.path ||
    sha(readFileSync(join(input, file.path))) !== file.sha256
  )
    throw new Error('Candidate input mismatch');
}
mkdirSync(results, { mode: 0o700 });
mkdirSync(tmp, { mode: 0o700 });
const binary = join(input, 'backend-tests');
const report = {
  status: 'INCOMPLETE',
  appVersion: manifest.appVersion,
  candidate: manifest,
  scope:
    manifest.mode === 'watermark'
      ? 'Backend regression and 100 generated 12MP watermark derivatives, stop/reopen/resume and per-file checks; no native GUI or total-app memory claim'
      : manifest.mode === 'batch'
        ? 'Backend regression and generated 100-photo 12MP copy, preparation, paused move/resume/withdrawal workload; no GUI, rendered watermark batch, installer or total-app-memory claim'
        : manifest.mode === 'regression'
          ? 'Backend regression using generated temporary fixtures only; no GUI, installer, benchmark or soak claim'
          : manifest.mode === 'baseline'
            ? 'Backend regression and generated 12MP cold/warm baseline; no GUI, WebView/GPU, IPC or system-cache isolation claim'
            : 'Backend regression and 30-minute 12MP decode/cache/RSS soak; no GUI, WebView/GPU, IPC, installer, or system-cache isolation claim',
  started: new Date().toISOString(),
  node: process.version,
  platform: process.platform,
  arch: process.arch,
  os: execFileSync('/usr/bin/sw_vers', ['-productVersion'], {
    encoding: 'utf8',
    timeout: 5000,
  }).trim(),
  cases: [],
};
function record(text, label) {
  const matches = [...text.matchAll(new RegExp(`(?:^|\\s)${label} (\\{[^\\r\\n]+\\})`, 'gm'))];
  if (matches.length !== 1) throw new Error(`Expected exactly one ${label} record`);
  return JSON.parse(matches[0][1]);
}
async function run(name, args, timeoutSeconds, monitor = false, commandBinary = binary) {
  const started = Date.now(),
    samples = [];
  let stdout = '',
    stderr = '',
    watchdog = false,
    overflow = false,
    memoryLimit = false,
    memoryError = null,
    spawnError = null,
    soakStarted = null;
  const child = spawn(commandBinary, args, {
    cwd: input,
    env: { ...process.env, TMPDIR: `${tmp}/` },
    detached: true,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  writeFileSync(
    join(root, 'progress.json'),
    JSON.stringify({ status: 'RUNNING', phase: name, started: new Date(started).toISOString() }),
    { mode: 0o600 },
  );
  let hardTimer,
    stopping = false;
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
  }, timeoutSeconds * 1000);
  const collect = () => {
    if (!child.pid) return;
    try {
      const footprintMode = manifest.mode === 'watermark' && name === 'watermark';
      if (footprintMode) {
        const usage = JSON.parse(
          execFileSync(join(input, 'app-memory'), [String(child.pid), commandBinary, '--single'], {
            encoding: 'utf8',
            timeout: 2000,
          }),
        );
        if (
          !usage.mainSeen ||
          usage.members !== 1 ||
          usage.unreadableMembers ||
          !Number.isFinite(usage.footprintBytes) ||
          usage.footprintBytes <= 0
        )
          throw Error('Incomplete memory observation');
        samples.push({
          elapsed: (Date.now() - started) / 1000,
          rssMiB: usage.rssBytes / 1048576,
          footprintMiB: usage.footprintBytes / 1048576,
        });
        writeFileSync(join(results, `${name}.rss.json`), JSON.stringify(samples, null, 2), {
          mode: 0o600,
        });
        writeFileSync(
          join(root, 'progress.json'),
          JSON.stringify({
            status: 'RUNNING',
            phase: name,
            ...samples.at(-1),
            samples: samples.length,
          }),
          { mode: 0o600 },
        );
        if (usage.footprintBytes > 512 * 1048576) {
          memoryLimit = true;
          stop();
        }
        return;
      }
      const value = execFileSync('/bin/ps', ['-o', 'rss=', '-p', String(child.pid)], {
        encoding: 'utf8',
        timeout: 2000,
      }).trim();
      if (!/^\d+$/.test(value)) return;
      const rssMiB = Number(value) / 1024;
      samples.push({
        elapsed: (Date.now() - started) / 1000,
        soakSeconds: soakStarted === null ? null : (Date.now() - soakStarted) / 1000,
        rssMiB,
      });
      writeFileSync(
        join(root, 'progress.json'),
        JSON.stringify({
          status: 'RUNNING',
          phase: name,
          ...samples.at(-1),
          samples: samples.length,
        }),
        { mode: 0o600 },
      );
      writeFileSync(join(results, `${name}.rss.json`), JSON.stringify(samples, null, 2), {
        mode: 0o600,
      });
      if (rssMiB > 1024) {
        memoryLimit = true;
        stop();
      }
    } catch (e) {
      // A process may exit between the timer and sampler. An alive but unreadable
      // watermark process must fail observation rather than silently losing evidence.
      if (manifest.mode === 'watermark' && name === 'watermark') {
        try {
          process.kill(child.pid, 0);
          memoryError = String(e);
          stop();
        } catch {}
      }
    }
  };
  const monitorTimer = monitor
    ? setInterval(collect, manifest.mode === 'watermark' ? 250 : 5000)
    : null;
  child.stdout.on('data', (chunk) => {
    stdout += chunk;
    appendFileSync(join(results, `${name}.stdout.log`), chunk, { mode: 0o600 });
    if (soakStarted === null && stdout.includes('HD_BASELINE {')) soakStarted = Date.now();
    if (stdout.length > 10_000_000) {
      overflow = true;
      stop();
    }
  });
  child.stderr.on('data', (chunk) => {
    stderr += chunk;
    appendFileSync(join(results, `${name}.stderr.log`), chunk, { mode: 0o600 });
    if (stderr.length > 10_000_000) {
      overflow = true;
      stop();
    }
  });
  child.on('error', (e) => {
    spawnError = e.message;
  });
  const outcome = await new Promise((resolve) =>
    child.on('close', (code, signal) => resolve({ code, signal })),
  );
  clearTimeout(timer);
  if (hardTimer) clearTimeout(hardTimer);
  if (monitorTimer) clearInterval(monitorTimer);
  let residualGroup = false;
  if (child.pid) {
    try {
      process.kill(-child.pid, 0);
      residualGroup = true;
    } catch (e) {
      if (e.code !== 'ESRCH') residualGroup = 'unknown';
    }
  }
  writeFileSync(join(results, `${name}.stdout.log`), stdout, { mode: 0o600 });
  writeFileSync(join(results, `${name}.stderr.log`), stderr, { mode: 0o600 });
  if (monitor)
    writeFileSync(join(results, `${name}.rss.json`), JSON.stringify(samples, null, 2), {
      mode: 0o600,
    });
  const result = {
    name,
    command: [commandBinary, ...args],
    started: new Date(started).toISOString(),
    finished: new Date().toISOString(),
    ...outcome,
    watchdog,
    overflow,
    memoryLimit,
    memoryError,
    spawnError,
    residualGroup,
    sampleCount: samples.length,
  };
  report.cases.push(result);
  if (
    result.code !== 0 ||
    result.signal !== null ||
    watchdog ||
    overflow ||
    memoryLimit ||
    memoryError ||
    spawnError ||
    residualGroup
  )
    throw new Error(`${name} process conditions failed`);
  return { stdout, samples };
}
try {
  const backend = await run('backend', ['--test-threads=2'], 120);
  const counts = backend.stdout.match(/test result: ok\. (\d+) passed; 0 failed; (\d+) ignored;/);
  if (
    !counts ||
    Number(counts[1]) !== manifest.expectedPassed ||
    Number(counts[2]) !== manifest.expectedIgnored
  )
    throw new Error('Backend count mismatch');
  report.backendPassed = Number(counts[1]);
  if (manifest.mode === 'watermark') {
    const result = await run(
      'watermark',
      [
        '--exact',
        'benchmarks::batch::hundred_watermark_derivatives',
        '--ignored',
        '--nocapture',
        '--test-threads=1',
      ],
      240,
      true,
    );
    report.watermark = record(result.stdout, 'WATERMARK_BATCH');
    report.watermarkRss = {
      samples: result.samples.length,
      maxMiB: Math.max(...result.samples.map((s) => s.rssMiB)),
      note: 'Observed RSS includes reclaimable CoreGraphics bitmap cache; not the physical footprint budget',
    };
    report.watermarkFootprint = {
      metric: 'proc_pid_rusage ri_phys_footprint',
      intervalMs: 250,
      budgetMiB: 512,
      maxMiB: Math.max(...result.samples.map((s) => s.footprintMiB)),
    };
    if (
      !/test result: ok\. 1 passed; 0 failed; 0 ignored;/.test(result.stdout) ||
      report.watermark.verified !== 100 ||
      report.watermark.outputs !== 201 ||
      report.watermark.originals_ids_ratings_pending_unchanged !== true ||
      result.samples.length < 1 ||
      !Number.isFinite(report.watermarkFootprint.maxMiB) ||
      report.watermarkFootprint.maxMiB > 512
    )
      throw Error('Watermark generation workload acceptance failed');
  } else if (manifest.mode === 'batch') {
    const batch = await run(
      'batch',
      [
        '--exact',
        'benchmarks::batch::hundred_photo_workflow',
        '--ignored',
        '--nocapture',
        '--test-threads=1',
      ],
      180,
      true,
      join(input, 'batch-tests'),
    );
    if (!/test result: ok\. 1 passed; 0 failed; 0 ignored;/.test(batch.stdout))
      throw Error('Batch workload did not complete');
    report.batch = record(batch.stdout, 'BATCH_PERF');
    report.batchRss = {
      samples: batch.samples.length,
      maxMiB: Math.max(...batch.samples.map((s) => s.rssMiB)),
    };
    if (
      report.batch.photos !== 100 ||
      report.batch.build !== 'release' ||
      report.batch.verified_copies !== 100 ||
      report.batch.verified_restored !== 100 ||
      report.batch.originals_ids_ratings_and_sidecars_unchanged !== true ||
      batch.samples.length < 1 ||
      report.batchRss.maxMiB > 512
    )
      throw Error('Batch workload acceptance failed');
  } else if (manifest.mode === 'baseline') {
    const baseline = await run(
      'baseline',
      [
        '--exact',
        'benchmarks::high_resolution_cold_and_warm_baseline',
        '--ignored',
        '--nocapture',
        '--test-threads=1',
      ],
      240,
    );
    if (!/test result: ok\. 1 passed; 0 failed; 0 ignored;/.test(baseline.stdout))
      throw new Error('Expected one completed baseline');
    report.baseline = record(baseline.stdout, 'HD_BASELINE');
    if (report.baseline.files !== 8 || report.baseline.build !== 'release')
      throw new Error('Wrong baseline workload or build');
  } else if (manifest.mode === 'soak') {
    const hd = await run(
      'hd',
      [
        '--exact',
        'benchmarks::high_resolution_preview_soak',
        '--ignored',
        '--nocapture',
        '--test-threads=1',
      ],
      2160,
      true,
    );
    if (!/test result: ok\. 1 passed; 0 failed; 0 ignored;/.test(hd.stdout))
      throw new Error('Expected one completed soak');
    report.baseline = record(hd.stdout, 'HD_BASELINE');
    report.soak = record(hd.stdout, 'HD_SOAK');
    if (
      report.soak.seconds < 1800 ||
      report.soak.files !== 8 ||
      report.soak.cache_bytes > 256 * 1024 * 1024 ||
      report.soak.original_checksums_unchanged !== true
    )
      throw new Error('Soak contract failed');
    const median = (values) => {
      const v = values.toSorted((a, b) => a - b);
      return v[Math.floor(v.length / 2)];
    };
    const warm = hd.samples.filter(
      (s) => s.soakSeconds !== null && s.soakSeconds >= 180 && s.soakSeconds < 480,
    );
    const late = hd.samples.filter((s) => s.soakSeconds >= 1500 && s.soakSeconds <= 1800);
    if (warm.length < 40 || late.length < 40 || hd.samples.length < 350)
      throw new Error('Insufficient RSS observations');
    const earlyMedianMiB = median(warm.map((s) => s.rssMiB)),
      lateMedianMiB = median(late.map((s) => s.rssMiB));
    const medianGrowthMiB = lateMedianMiB - earlyMedianMiB,
      allowedGrowthMiB = Math.max(64, earlyMedianMiB * 0.1);
    report.memory = {
      metric: 'backend process RSS from /bin/ps every 5s',
      samples: hd.samples.length,
      peakMiB: Math.max(...hd.samples.map((s) => s.rssMiB)),
      earlyWindowSeconds: [180, 480],
      lateWindowSeconds: [1500, 1800],
      earlyMedianMiB,
      lateMedianMiB,
      medianGrowthMiB,
      allowedGrowthMiB,
      peakLimitMiB: 1024,
      withinBudget: medianGrowthMiB <= allowedGrowthMiB,
    };
    if (!report.memory.withinBudget)
      throw new Error('RSS window growth exceeds the provisional budget');
  }
  report.status = 'PASS';
} catch (e) {
  report.status = 'FAIL';
  report.error = String(e.message ?? e);
}
report.finished = new Date().toISOString();
writeFileSync(
  join(root, 'progress.json'),
  JSON.stringify({
    status: report.status,
    phase: 'complete',
    finished: report.finished,
    error: report.error,
  }),
  { mode: 0o600 },
);
writeFileSync(join(results, 'report.json'), JSON.stringify(report, null, 2), { mode: 0o600 });
const outputs = readdirSync(results)
  .sort()
  .map((path) => ({
    path,
    bytes: statSync(join(results, path)).size,
    sha256: sha(readFileSync(join(results, path))),
  }));
const outputManifest = JSON.stringify(outputs, null, 2);
writeFileSync(join(results, 'files.json'), outputManifest, { mode: 0o600 });
console.log(
  JSON.stringify({
    status: report.status,
    backendPassed: report.backendPassed,
    baseline: report.baseline,
    soak: report.soak,
    batch: report.batch,
    batchRss: report.batchRss,
    watermark: report.watermark,
    watermarkRss: report.watermarkRss,
    watermarkFootprint: report.watermarkFootprint,
    memory: report.memory,
    error: report.error,
    outputManifestSha256: sha(outputManifest),
  }),
);
process.exitCode = report.status === 'PASS' ? 0 : 1;
