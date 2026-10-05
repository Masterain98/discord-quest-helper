/**
 * sync-version.js
 *
 * Reads the canonical version from public/version.txt, strips any
 * pre-release suffix (e.g. -rc1, -beta2), and patches:
 *   - package.json           (npm "version" field)
 *   - src-tauri/Cargo.toml   (Cargo [package] version)
 *   - Cargo.lock             (only this workspace package's version)
 *   - src-tauri/tauri.conf.json (Tauri "version" field)
 *
 * This ensures the Windows PE binary's FILEVERSION / PRODUCTVERSION
 * and the Rust env!("CARGO_PKG_VERSION") always match the real release
 * version (minus pre-release tags).
 *
 * Run automatically before sidecar builds in tauri:dev / tauri:build
 * and the release/manual-build workflows.
 */

import { readFileSync, writeFileSync } from 'fs';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';
import { bundleVersion, syncCargoLockVersion } from './release-version.mjs';

const __dirname = dirname(fileURLToPath(import.meta.url));
const rootDir = resolve(__dirname, '..');

// ── 1. Read canonical version ────────────────────────────────────────────────
const versionFile = resolve(rootDir, 'public', 'version.txt');
const rawVersion = readFileSync(versionFile, 'utf-8').trim();

if (!rawVersion) {
    console.error('❌ public/version.txt is empty');
    process.exit(1);
}

// Strip pre-release suffix: "0.9.0-rc2" → "0.9.0"
const semver = bundleVersion(rawVersion);

console.log(`🔄 Syncing version: ${rawVersion} → ${semver}`);

// Update only the local package version. Keep every dependency resolution and
// checksum intact so subsequent --locked builds still enforce reproducibility.
const cargoPath = resolve(rootDir, 'src-tauri', 'Cargo.toml');
const cargo = readFileSync(cargoPath, 'utf-8');
const packageName = /^name\s*=\s*"([^"]+)"/m.exec(cargo)?.[1];
const lockPath = resolve(rootDir, 'Cargo.lock');
const updatedLock = syncCargoLockVersion(readFileSync(lockPath, 'utf-8'), packageName, semver);

// ── 2. Patch package.json ────────────────────────────────────────────────────
const pkgPath = resolve(rootDir, 'package.json');
const pkg = readFileSync(pkgPath, 'utf-8');
const updatedPkg = pkg.replace(
    /("version"\s*:\s*")[\d.]+(")/,
    `$1${semver}$2`
);
writeFileSync(pkgPath, updatedPkg);
console.log(`   ✅ package.json → ${semver}`);

// ── 3. Patch src-tauri/Cargo.toml ────────────────────────────────────────────
const updatedCargo = cargo.replace(
    /^(version\s*=\s*)"[^"]+"/m,
    `$1"${semver}"`
);
writeFileSync(cargoPath, updatedCargo);
console.log(`   ✅ src-tauri/Cargo.toml → ${semver}`);
writeFileSync(lockPath, updatedLock);
console.log(`   ✅ Cargo.lock workspace package → ${semver}`);

// ── 4. Patch src-tauri/tauri.conf.json ───────────────────────────────────────
const tauriConfPath = resolve(rootDir, 'src-tauri', 'tauri.conf.json');
const tauriConf = readFileSync(tauriConfPath, 'utf-8');
const updatedTauriConf = tauriConf.replace(
    /("version"\s*:\s*")[\d.]+(")/,
    `$1${semver}$2`
);
writeFileSync(tauriConfPath, updatedTauriConf);
console.log(`   ✅ src-tauri/tauri.conf.json → ${semver}`);

console.log('🎉 Version sync complete.');
