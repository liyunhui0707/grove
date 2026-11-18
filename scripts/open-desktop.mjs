// A fixed project entry, not a bundle-name search (which may pick an older App).
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { root } from './toolchain.mjs';

export function trialLaunchArguments(app, channel, home) {
  if (!['local-trial', 'public-prerelease'].includes(channel)) {
    throw Error('Only accepted trial or public prerelease candidates can use the trial entry');
  }
  // Match the existing local-trial bundle's stable catalog, even when the
  // accepted binary comes from the public download. Never use the daily scope.
  const data = path.join(home, 'Library/Application Support/app.momentgrove.trial');
  if (!path.isAbsolute(app) || !path.isAbsolute(data)) throw Error('Absolute paths required');
  // Deliver the scope even if another app with the public bundle ID is open.
  // The backend catalog lock forwards repeated trial launches to its owner.
  return ['-n', '--env', `MOMENT_GROVE_DATA_DIR=${data}`, app];
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  const args = process.argv.slice(2);
  if (args.length > 1 || args.some((a) => a !== '--info')) throw Error('Only --info is supported');
  const payload = fs.realpathSync(path.join(root, '.local/current-trial'));
  const app = path.join(
    payload,
    fs.existsSync(path.join(payload, 'Grove.app')) ? 'Grove.app' : 'Moment Grove.app',
  );
  const candidate = JSON.parse(fs.readFileSync(path.join(payload, 'candidate.json'), 'utf8'));
  const binary = candidate.product === 'Grove' ? 'grove' : 'moment-grove';
  fs.accessSync(path.join(app, 'Contents/MacOS', binary), fs.constants.X_OK);
  const launchArgs = trialLaunchArguments(app, candidate.channel, os.homedir());
  console.log(
    JSON.stringify(
      {
        app,
        version: candidate.version,
        buildId: candidate.buildId,
        channel: candidate.channel,
        dataScope: 'independent-trial',
        dataDirectory: launchArgs[2].slice('MOMENT_GROVE_DATA_DIR='.length),
      },
      null,
      2,
    ),
  );
  if (!args.includes('--info')) {
    if (process.platform !== 'darwin') throw Error('This entry supports macOS only');
    execFileSync('/usr/bin/open', launchArgs, { stdio: 'inherit' });
  }
}
