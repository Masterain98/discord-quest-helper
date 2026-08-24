import { execFileSync } from 'child_process';
import { dirname, join, resolve } from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const rootDir = resolve(dirname(__filename), '..');
const manifestPath = join(rootDir, 'Cargo.toml');
const metadata = JSON.parse(execFileSync(
  'cargo',
  ['metadata', '--format-version', '1', '--no-deps', '--manifest-path', manifestPath],
  { cwd: rootDir, encoding: 'utf8' },
));
const corePackage = metadata.packages.find(pkg => pkg.name === 'discord-cdp-launch-core');
if (!corePackage) throw new Error('discord-cdp-launch-core package metadata is unavailable.');
const forbiddenDirect = corePackage.dependencies
  .map(dependency => dependency.name)
  .filter(name => /^tauri(?:$|-)/.test(name));
if (forbiddenDirect.length > 0) {
  throw new Error(`discord-cdp-launch-core declares a Tauri dependency: ${forbiddenDirect.join(', ')}`);
}

// Resolve the current host's complete transitive tree. CI runs this same gate
// on Windows, macOS, and Linux, covering every supported target without making
// each job download all other platforms' conditional dependency graphs.
const output = execFileSync(
  'cargo',
  [
    'tree',
    '--manifest-path', manifestPath,
    '--package', 'discord-cdp-launch-core',
    '--all-features',
    '--prefix', 'none',
  ],
  { cwd: rootDir, encoding: 'utf8' },
);

const forbidden = output
  .split(/\r?\n/)
  .map(line => line.trim())
  .filter(line => /^tauri(?:\s|-)/.test(line));

if (forbidden.length > 0) {
  throw new Error(`discord-cdp-launch-core must not depend on Tauri:\n${forbidden.join('\n')}`);
}

console.log('discord-cdp-launch-core dependency tree is Tauri-free.');
