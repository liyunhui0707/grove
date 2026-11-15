// A fixed project entry, not a bundle-name search (which may pick an older App).
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { root } from './toolchain.mjs';

const args = process.argv.slice(2);
if (args.length > 1 || args.some((a) => a !== '--info')) throw Error('Only --info is supported');
const payload = fs.realpathSync(path.join(root, '.local/current-trial'));
const app = path.join(payload, 'Moment Grove.app');
const candidate = JSON.parse(fs.readFileSync(path.join(payload, 'candidate.json'), 'utf8'));
fs.accessSync(path.join(app, 'Contents/MacOS/moment-grove'), fs.constants.X_OK);
console.log(
  JSON.stringify(
    { app, version: candidate.version, buildId: candidate.buildId, channel: candidate.channel },
    null,
    2,
  ),
);
if (!args.includes('--info')) {
  if (process.platform !== 'darwin') throw Error('This entry supports macOS only');
  // Normal LaunchServices reopen, not forced creation of a second process.
  execFileSync('/usr/bin/open', [app], { stdio: 'inherit' });
}
