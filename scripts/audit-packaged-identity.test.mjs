import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import {
  auditArtifact,
  containsProductToken,
  IDENTITY,
  parseCodeIdentity,
  pngDimensions,
  relatedCodeIdentityViolations,
  validateInternalName,
} from './audit-packaged-identity.mjs';

test('macOS code identity parsing and relationship policy bind the helper team', () => {
  const main = parseCodeIdentity(`Identifier=com.example.app
Authority=Developer ID Application: Example (ABC123)
TeamIdentifier=ABC123
flags=0x10000(runtime)`);
  const helper = parseCodeIdentity(`Identifier=waybridge
Authority=Developer ID Application: Example (ABC123)
TeamIdentifier=ABC123
flags=0x10000(runtime)`);
  assert.deepEqual(relatedCodeIdentityViolations(main, helper, false), []);

  const wrongTeam = { ...helper, teamIdentifier: 'XYZ999' };
  assert.ok(relatedCodeIdentityViolations(main, wrongTeam, false)
    .includes('runtime bridge TeamIdentifier does not match the main app'));
});

test('macOS smoke policy accepts only hardened ad-hoc app and helper identities', () => {
  const adHoc = parseCodeIdentity('Identifier=fixture\nTeamIdentifier=not set\nSignature=adhoc\nflags=0x10000(runtime)');
  assert.deepEqual(relatedCodeIdentityViolations(adHoc, adHoc, true), []);
  const unsigned = parseCodeIdentity('Identifier=fixture');
  assert.notDeepEqual(relatedCodeIdentityViolations(adHoc, unsigned, true), []);
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
    allowAdHoc: false,
  });
  assert.equal(manifest.passed, true);
  assert.equal(manifest.mainBinary, IDENTITY.mainBinary);
  assert.equal(manifest.bridgeBinary, IDENTITY.bridgeBinary);
  assert.equal(manifest.hashes[IDENTITY.bridgeBinary].length, 64);
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
