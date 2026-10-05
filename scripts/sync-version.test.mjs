import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { bundleVersion, syncCargoLockVersion } from './release-version.mjs';

const scripts = dirname(fileURLToPath(import.meta.url));

test('bundle versions normalize candidates while retaining the exact provenance version', () => {
  for (const release of ['0.10.8', '0.10.8-rc1', '0.10.8-rc2\r\n']) {
    assert.equal(bundleVersion(release), '0.10.8');
  }
  assert.throws(() => bundleVersion('invalid'), /Invalid release version/);
});

test('lockfile synchronization leaves dependency versions, sources and checksums intact', () => {
  const lock = '# generated\nversion = 4\n\n[[package]]\nname = "app"\nversion = "0.1.0"\n\n'
    + '[[package]]\nname = "dependency"\nversion = "0.1.0"\nsource = "registry+fixture"\nchecksum = "unchanged"\n';
  const updated = syncCargoLockVersion(lock, 'app', '0.2.0');
  assert.equal(updated.replace('name = "app"\nversion = "0.2.0"', 'name = "app"\nversion = "0.1.0"'), lock);
  assert.equal(syncCargoLockVersion(updated, 'app', '0.2.0'), updated);
  assert.throws(() => syncCargoLockVersion(lock, 'missing', '0.2.0'), /Expected one workspace/);
  assert.throws(() => syncCargoLockVersion(lock, 'dependency', '0.2.0'), /found 0/);
});

test('changing only public/version.txt permits locked sidecar builds for stable and RC releases', (context) => {
  const root = mkdtempSync(join(tmpdir(), 'version-sync-'));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  for (const directory of ['scripts', 'public', 'src-tauri/src', 'src-runner/src']) {
    mkdirSync(join(root, directory), { recursive: true });
  }
  for (const name of ['sync-version.js', 'release-version.mjs']) copyFileSync(join(scripts, name), join(root, 'scripts', name));
  writeFileSync(join(root, 'package.json'), '{"type":"module","version":"0.10.7"}\n');
  writeFileSync(join(root, 'src-tauri/tauri.conf.json'), '{"version":"0.10.7"}\n');
  writeFileSync(join(root, 'Cargo.toml'), '[workspace]\nmembers = ["src-tauri", "src-runner"]\nresolver = "2"\n');
  writeFileSync(join(root, 'src-tauri/Cargo.toml'), '[package]\nname = "app"\nversion = "0.10.7"\nedition = "2021"\n');
  writeFileSync(join(root, 'src-tauri/src/lib.rs'), 'pub fn fixture() {}\n');
  writeFileSync(join(root, 'src-runner/Cargo.toml'), '[package]\nname = "runner"\nversion = "0.1.0"\nedition = "2021"\n');
  writeFileSync(join(root, 'src-runner/src/main.rs'), 'fn main() {}\n');
  // The lockfile starts in sync; only the canonical release file changes.
  const cargo = (args) => execFileSync('cargo', args, { cwd: root, env: { ...process.env, CARGO_TARGET_DIR: join(root, 'target') }, stdio: 'pipe' });
  cargo(['generate-lockfile', '--offline']);
  for (const release of ['0.10.8', '0.10.9-rc1']) {
    writeFileSync(join(root, 'public/version.txt'), `${release}\n`);
    const version = bundleVersion(release);
    execFileSync(process.execPath, ['scripts/sync-version.js'], { cwd: root });
    assert.equal(JSON.parse(readFileSync(join(root, 'package.json'))).version, version);
    assert.equal(JSON.parse(readFileSync(join(root, 'src-tauri/tauri.conf.json'))).version, version);
    assert.match(readFileSync(join(root, 'src-tauri/Cargo.toml'), 'utf8'), new RegExp(`version = "${version}"`));
    const lockfile = readFileSync(join(root, 'Cargo.lock'), 'utf8');
    cargo(['build', '--locked', '--offline', '--package', 'runner']);
    assert.equal(readFileSync(join(root, 'Cargo.lock'), 'utf8'), lockfile, 'locked build must not rewrite dependencies');
  }
});
