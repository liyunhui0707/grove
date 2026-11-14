import { spawn } from 'node:child_process';
import path from 'node:path';
import { root, toolchainEnv } from './toolchain.mjs';
const cli = path.join(root, 'node_modules/.bin/tauri');
const child = spawn(cli, process.argv.slice(2), {
  cwd: path.join(root, 'apps/desktop'),
  env: toolchainEnv(),
  stdio: 'inherit',
});
child.on('error', (e) => {
  console.error(e.message);
  process.exitCode = 1;
});
child.on('exit', (code) => {
  process.exitCode = code ?? 1;
});
