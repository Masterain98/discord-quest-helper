import { execFileSync } from 'child_process';
import { dirname, join, resolve } from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const rootDir = resolve(dirname(__filename), '..');
const output = execFileSync(
  'cargo',
  [
    'tree',
    '--manifest-path', join(rootDir, 'Cargo.toml'),
    '--package', 'discord-cdp-launch-core',
    '--all-features',
    '--target', 'all',
    '--prefix', 'none',
  ],
  { cwd: rootDir, encoding: 'utf8' },
);

const forbidden = output
  .split(/\r?\n/)
  .map(line => line.trim())
  .filter(line => line.startsWith('tauri ') || line.startsWith('tauri-plugin-'));

if (forbidden.length > 0) {
  throw new Error(`discord-cdp-launch-core must not depend on Tauri:\n${forbidden.join('\n')}`);
}

console.log('discord-cdp-launch-core dependency tree is Tauri-free.');
