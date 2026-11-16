import assert from 'node:assert/strict';
import { before, test } from 'node:test';
import { execFileSync, spawnSync, spawn } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const mac = process.platform === 'darwin';
let binary;
before(() => {
  if (!mac) return;
  const directory = mkdtempSync(path.join(tmpdir(), 'moment-grove-memory-check-'));
  binary = path.join(directory, 'app-memory');
  const source = fileURLToPath(new URL('../qa/app-memory.c', import.meta.url));
  execFileSync('/usr/bin/clang', ['-Wall', '-Wextra', '-Werror', '-O2', source, '-o', binary]);
});
const sample = (extra = [], executable = process.execPath, pid = process.pid) =>
  spawnSync(binary, [String(pid), executable, ...extra], { encoding: 'utf8', timeout: 5000 });

test(
  'memory observation refuses an unrelated executable and invalid process IDs',
  { skip: !mac },
  () => {
    assert.equal(sample(['--single'], '/usr/bin/true').status, 3);
    for (const pid of [0, -1, 'invalid'])
      assert.equal(sample(['--single'], process.execPath, pid).status, 2);
    assert.equal(sample(['--unknown']).status, 2);
  },
);

test(
  'single-process sampling retains positive native memory and the exact target',
  { skip: !mac },
  () => {
    const result = sample(['--single']);
    assert.equal(result.status, 0, result.stderr);
    const data = JSON.parse(result.stdout);
    assert.equal(data.members, 1);
    assert.equal(data.mainSeen, true);
    assert.equal(data.processes[0].pid, process.pid);
    assert.ok(data.rssBytes > 0 && data.footprintBytes > 0);
  },
);

test(
  'WebKit scope excludes a same-coalition QA worker and does not pass without WebKit',
  { skip: !mac },
  async () => {
    const worker = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], {
      stdio: 'ignore',
    });
    try {
      await new Promise((resolve, reject) => {
        worker.once('spawn', resolve);
        worker.once('error', reject);
      });
      const result = sample(['--webview']);
      assert.ok([0, 6].includes(result.status), result.stderr);
      const data = JSON.parse(result.stdout);
      assert.equal(data.mainSeen, true);
      assert.ok(!data.processes.some((p) => p.pid === worker.pid));
      assert.equal(data.members, data.webViewProcesses + 1);
      assert.match(data.method, /excludes other services/);
      if (!data.webViewProcesses) assert.equal(result.status, 6);
      else if (!data.unreadableMembers) assert.equal(result.status, 0);
    } finally {
      worker.kill('SIGTERM');
      await new Promise((resolve) => worker.once('close', resolve));
    }
  },
);
