import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import {
  auditArtifact,
  containsProductToken,
  IDENTITY,
  validateInternalName,
} from './audit-packaged-identity.mjs';

test('configured artifact identities satisfy the stable naming policy', () => {
  assert.equal(validateInternalName(IDENTITY.mainBinary, 'meridian'), true);
  assert.equal(validateInternalName(IDENTITY.bridgeBinary, 'waybridge'), true);
  assert.equal(validateInternalName(IDENTITY.runnerBuildBinary, 'stagecraft'), true);
});

test('product names and random-looking hex names fail internal validation', () => {
  assert.equal(validateInternalName('discord-quest-helper', 'discord-quest-helper'), false);
  assert.equal(validateInternalName('abcdef123456', 'abcdef123456'), false);
});

test('public identity remains allowed outside internal executable metadata', () => {
  assert.equal(IDENTITY.publicName, 'Discord Quest Helper');
  assert.equal(containsProductToken(IDENTITY.publicName), true);
});

test('Linux AppDir audit requires desktop integration with the neutral runtime', (context) => {
  const appDir = mkdtempSync(join(tmpdir(), 'identity-appdir-'));
  context.after(() => rmSync(appDir, { recursive: true, force: true }));
  const binDir = join(appDir, 'usr', 'bin');
  const desktopDir = join(appDir, 'usr', 'share', 'applications');
  mkdirSync(binDir, { recursive: true });
  mkdirSync(desktopDir, { recursive: true });
  const main = join(binDir, IDENTITY.mainBinary);
  writeFileSync(main, 'fixture');
  chmodSync(main, 0o755);
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
    allowUnsigned: false,
  });
  assert.equal(manifest.passed, true);
  assert.equal(manifest.mainBinary, IDENTITY.mainBinary);
});
