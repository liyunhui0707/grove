import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

export const root = fileURLToPath(new URL('../', import.meta.url));
export function toolchainEnv() {
  const home = path.join(root, '.local');
  const env = { ...process.env };
  if (existsSync(path.join(home, 'cargo/bin/cargo'))) {
    env.CARGO_HOME = path.join(home, 'cargo');
    env.RUSTUP_HOME = path.join(home, 'rustup');
    env.PATH = `${path.join(home, 'cargo/bin')}${path.delimiter}${env.PATH ?? ''}`;
  }
  return env;
}
