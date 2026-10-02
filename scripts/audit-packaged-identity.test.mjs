import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import {
  auditArtifact,
  containsProductToken,
  IDENTITY,
  incompatibleAppImageLibraries,
  MACOS_SIGNING_ENABLED,
  parseCodeIdentity,
  pngDimensions,
  relatedCodeIdentityViolations,
  validateInternalName,
} from './audit-packaged-identity.mjs';

test('macOS signing remains disabled even though dormant verification helpers stay available', () => {
  assert.equal(MACOS_SIGNING_ENABLED, false);
});

test('macOS policy accepts only hardened ad-hoc app and helper identities', () => {
  const adHoc = parseCodeIdentity('Identifier=fixture\nTeamIdentifier=not set\nSignature=adhoc\nflags=0x10000(runtime)');
  assert.deepEqual(relatedCodeIdentityViolations(adHoc, adHoc), []);
  const unsigned = parseCodeIdentity('Identifier=fixture');
  assert.notDeepEqual(relatedCodeIdentityViolations(adHoc, unsigned), []);
});

test('configured artifact identities satisfy the stable naming policy', () => {
  assert.equal(validateInternalName(IDENTITY.mainBinary, 'meridian'), true);
  assert.equal(validateInternalName(IDENTITY.bridgeBinary, 'waybridge'), true);
  assert.equal(validateInternalName(IDENTITY.runnerBuildBinary, 'stagecraft'), true);
});

test('product names and random-looking hex names fail internal validation', () => {
  assert.equal(validateInternalName('discord-quest-helper', 'discord-quest-helper'), false);
  assert.equal(validateInternalName('abcdef123456', 'abcdef123456'), false);
  assert.equal(validateInternalName('deadbeef', 'deadbeef'), false);
});

test('public identity remains allowed outside internal executable metadata', () => {
  assert.equal(IDENTITY.publicName, 'Discord Quest Helper');
  assert.equal(containsProductToken(IDENTITY.publicName), true);
});

test('AppImage audit detects bundled Wayland clients in flat and multiarch library paths', (context) => {
  const root = mkdtempSync(join(tmpdir(), 'appimage-libraries-'));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  const flat = join(root, 'usr', 'lib');
  const multiarch = join(flat, 'x86_64-linux-gnu');
  mkdirSync(multiarch, { recursive: true });
  writeFileSync(join(flat, 'libwayland-client.so.0'), 'old library');
  writeFileSync(join(multiarch, 'libwayland-client.so.0.20.0'), 'old library');
  writeFileSync(join(flat, 'libwebkit2gtk-4.1.so.0'), 'required bundled library');
  assert.deepEqual(incompatibleAppImageLibraries(root), [
    join('usr', 'lib', 'libwayland-client.so.0'),
    join('usr', 'lib', 'x86_64-linux-gnu', 'libwayland-client.so.0.20.0'),
  ]);
  rmSync(join(flat, 'libwayland-client.so.0'));
  rmSync(join(multiarch, 'libwayland-client.so.0.20.0'));
  assert.deepEqual(incompatibleAppImageLibraries(root), []);
});

test('AppImage audit detects Wayland client symlinks without following them', {
  skip: process.platform === 'win32' ? 'POSIX symlink fixture' : false,
}, (context) => {
  const root = mkdtempSync(join(tmpdir(), 'appimage-library-links-'));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  symlinkSync('/missing/libwayland-client.so.0', join(root, 'libwayland-client.so.0'));
  assert.deepEqual(incompatibleAppImageLibraries(root), ['libwayland-client.so.0']);
});

test('Linux AppDir audit requires desktop integration with the neutral runtime', {
  skip: process.platform === 'win32' ? 'POSIX executable mode fixture' : false,
}, (context) => {
  const appDir = mkdtempSync(join(tmpdir(), 'identity-appdir-'));
  context.after(() => rmSync(appDir, { recursive: true, force: true }));
  const binDir = join(appDir, 'usr', 'bin');
  const desktopDir = join(appDir, 'usr', 'share', 'applications');
  const iconDir = join(appDir, 'usr', 'share', 'icons', 'hicolor', '64x64', 'apps');
  mkdirSync(binDir, { recursive: true });
  mkdirSync(desktopDir, { recursive: true });
  mkdirSync(iconDir, { recursive: true });
  const main = join(binDir, IDENTITY.mainBinary);
  const bridge = join(binDir, IDENTITY.bridgeBinary);
  writeFileSync(main, 'fixture');
  writeFileSync(bridge, 'fixture');
  chmodSync(main, 0o755);
  chmodSync(bridge, 0o755);
  const pngHeader = Buffer.alloc(24);
  Buffer.from('89504e470d0a1a0a', 'hex').copy(pngHeader);
  pngHeader.writeUInt32BE(64, 16);
  pngHeader.writeUInt32BE(64, 20);
  writeFileSync(join(iconDir, 'com.masterain.discord-quest-helper.png'), pngHeader);
  writeFileSync(join(desktopDir, 'public.desktop'), `[Desktop Entry]
Name=Discord Quest Helper
Exec=meridian
Icon=com.masterain.discord-quest-helper
StartupWMClass=meridian
Terminal=false
Type=Application
`);

  const manifest = auditArtifact({
    platform: 'linux',
    artifact: appDir,
    kind: 'appdir',
  });
  assert.equal(manifest.passed, true);
  assert.equal(manifest.mainBinary, IDENTITY.mainBinary);
  assert.equal(manifest.bridgeBinary, IDENTITY.bridgeBinary);
  assert.equal(manifest.hashes[IDENTITY.bridgeBinary].length, 64);

  const libraries = join(appDir, 'usr', 'lib');
  mkdirSync(libraries, { recursive: true });
  writeFileSync(join(libraries, 'libwayland-client.so.0'), 'incompatible library');
  const incompatible = auditArtifact({ platform: 'linux', artifact: appDir, kind: 'appdir' });
  assert.equal(incompatible.passed, false);
  assert.ok(incompatible.violations.some((violation) => violation.includes('host Wayland client')));
});

test('PNG dimension audit distinguishes a 1x1 placeholder', (context) => {
  const directory = mkdtempSync(join(tmpdir(), 'identity-icon-'));
  context.after(() => rmSync(directory, { recursive: true, force: true }));
  const icon = join(directory, 'icon.png');
  const header = Buffer.alloc(24);
  Buffer.from('89504e470d0a1a0a', 'hex').copy(header);
  header.writeUInt32BE(1, 16);
  header.writeUInt32BE(1, 20);
  writeFileSync(icon, header);
  assert.deepEqual(pngDimensions(icon), { width: 1, height: 1 });
});
