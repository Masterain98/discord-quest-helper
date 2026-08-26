#!/usr/bin/env node

import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
function json(path) {
  return JSON.parse(readFileSync(join(ROOT, path), 'utf8'));
}

function text(path) {
  return readFileSync(join(ROOT, path), 'utf8');
}

function requireMatch(condition, message, failures) {
  if (!condition) failures.push(message);
}

export function validateConfiguration() {
  const failures = [];
  const identity = json('scripts/runtime-identity-tokens.json').identity;
  const expected = {
    main: identity.mainBinary,
    bridge: identity.bridgeBinary,
    runner: identity.runnerBuildBinary,
    publicMain: identity.publicBinary,
  };
  const common = json('src-tauri/tauri.conf.json');
  const linux = json('src-tauri/tauri.linux.conf.json');
  const macos = json('src-tauri/tauri.macos.conf.json');
  const launcherCargo = text('src-cdp-launcher/Cargo.toml');
  const runnerCargo = text('src-runner/Cargo.toml');
  const workspaceCargo = text('Cargo.toml');
  const releaseWorkflow = text('.github/workflows/build-release.yml');

  requireMatch(common.mainBinaryName === expected.publicMain,
    'common/Windows mainBinaryName must retain the public executable name', failures);
  requireMatch(linux.mainBinaryName === expected.main,
    `Linux mainBinaryName must be ${expected.main}`, failures);
  requireMatch(linux.app?.enableGTKAppId === false,
    'Linux must not expose the product identifier as the GTK app ID', failures);
  requireMatch(linux.bundle?.linux?.deb?.desktopTemplate === 'linux/discord-quest-helper.desktop.hbs',
    'Linux DEB must use the audited desktop entry template', failures);
  requireMatch(macos.mainBinaryName === expected.main,
    `macOS mainBinaryName must be ${expected.main}`, failures);
  requireMatch(macos.bundle?.macOS?.hardenedRuntime === true,
    'macOS hardenedRuntime must be enabled', failures);
  requireMatch(macos.bundle?.macOS?.signingIdentity === '-',
    'macOS builds must use the configured ad-hoc signing identity', failures);
  requireMatch(!/APPLE_(?:CERTIFICATE|SIGNING|API|KEYCHAIN)|REQUIRE_NOTARIZATION|notarytool|stapler/.test(releaseWorkflow),
    'release workflow must not require Apple signing or notarization credentials', failures);
  requireMatch(JSON.stringify(common.bundle?.externalBin) === JSON.stringify([`binaries/${expected.bridge}`]),
    `externalBin must contain only binaries/${expected.bridge}`, failures);
  requireMatch(new RegExp(`\\[\\[bin\\]\\][\\s\\S]*?name\\s*=\\s*"${expected.bridge}"`).test(launcherCargo),
    `launcher binary must be named ${expected.bridge}`, failures);
  requireMatch(new RegExp(`\\[\\[bin\\]\\][\\s\\S]*?name\\s*=\\s*"${expected.runner}"`).test(runnerCargo),
    `runner build binary must be named ${expected.runner}`, failures);
  requireMatch(/\[profile\.release\][\s\S]*?strip\s*=\s*"symbols"/.test(workspaceCargo),
    'release profile must strip public symbols', failures);
  requireMatch(/\[profile\.release\][\s\S]*?split-debuginfo\s*=\s*"packed"/.test(workspaceCargo),
    'release profile must emit separate packed debug symbols', failures);

  return failures;
}

const failures = validateConfiguration();
if (failures.length) {
  for (const failure of failures) console.error(`runtime identity config: ${failure}`);
  process.exitCode = 1;
} else {
  console.log('Runtime identity build configuration is consistent.');
}
