import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { linuxElfContentHash } from './sidecar-provenance.mjs';
import { linuxBundleInputViolations } from './check-linux-bundle-inputs.mjs';

import {
  auditArtifact,
  APPIMAGE_MEDIA_FILES,
  containsProductToken,
  IDENTITY,
  incompatibleAppImageLibraries,
  missingAppImageMediaFiles,
  MACOS_SIGNING_ENABLED,
  parseCodeIdentity,
  pngDimensions,
  relatedCodeIdentityViolations,
  squashfsOffset,
  sidecarProvenanceViolations,
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

function writeMediaFixture(root) {
  for (const file of APPIMAGE_MEDIA_FILES) {
    const path = join(root, file);
    mkdirSync(join(path, '..'), { recursive: true });
    writeFileSync(path, file.endsWith('.sh') ? 'media fixture' : elfFixture());
    chmodSync(path, 0o755);
  }
}

function elfFixture() {
  // Explicit SHT_NULL avoids older readelf treating e_shnum=0 as an extended
  // section count and reading the ELF header as section zero at e_shoff=0.
  const bytes = Buffer.alloc(184);
  Buffer.from([0x7f, 0x45, 0x4c, 0x46, 2, 1, 1]).copy(bytes);
  bytes.writeUInt16LE(3, 16);
  bytes.writeUInt16LE(62, 18);
  bytes.writeUInt32LE(1, 20);
  bytes.writeBigUInt64LE(64n, 32);
  bytes.writeBigUInt64LE(120n, 40);
  bytes.writeUInt16LE(64, 52);
  bytes.writeUInt16LE(56, 54);
  bytes.writeUInt16LE(1, 56);
  bytes.writeUInt16LE(64, 58);
  bytes.writeUInt16LE(1, 60);
  bytes.writeUInt32LE(1, 64); // PT_LOAD
  bytes.writeUInt32LE(5, 68); // read + execute
  bytes.writeBigUInt64LE(BigInt(bytes.length), 96);
  bytes.writeBigUInt64LE(BigInt(bytes.length), 104);
  bytes.writeBigUInt64LE(4096n, 112);
  return bytes;
}

test('Linux bundle inputs match the Ubuntu 22.04 NSS package layout', (context) => {
  const root = mkdtempSync(join(tmpdir(), 'identity-bundle-inputs-'));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  const config = JSON.parse(readFileSync(new URL('../src-tauri/tauri.linux.conf.json', import.meta.url), 'utf8'));
  // Paths verified against the actual Jammy libnss3 package, not derived from config.
  for (const source of ['usr/lib/x86_64-linux-gnu/nss/libsoftokn3.so', 'usr/lib/x86_64-linux-gnu/libfreeblpriv3.so']) {
    const path = join(root, source);
    mkdirSync(join(path, '..'), { recursive: true });
    writeFileSync(path, elfFixture());
  }
  assert.deepEqual(linuxBundleInputViolations(config, { sourceRoot: root }), []);
  config.bundle.linux.appimage.files['/usr/lib/libsoftokn3.so'] = '/usr/lib/x86_64-linux-gnu/libsoftokn3.so';
  assert.match(linuxBundleInputViolations(config, { sourceRoot: root })[0], /libsoftokn3\.so.*ENOENT/);
});

test('Linux bundle input check rejects missing, empty, non-ELF and wrong-architecture libraries', (context) => {
  const root = mkdtempSync(join(tmpdir(), 'identity-bundle-inputs-'));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  const config = { bundle: { linux: { appimage: { files: { '/usr/lib/fixture.so': '/fixture.so' } } } } };
  const check = () => linuxBundleInputViolations(config, { sourceRoot: root });
  assert.match(check()[0], /ENOENT/);
  writeFileSync(join(root, 'fixture.so'), '');
  assert.match(check()[0], /nonempty regular file/);
  writeFileSync(join(root, 'fixture.so'), 'not ELF');
  assert.match(check()[0], /x86_64 ELF64/);
  const bytes = elfFixture();
  bytes.writeUInt16LE(183, 18);
  writeFileSync(join(root, 'fixture.so'), bytes);
  assert.match(check()[0], /x86_64 ELF64/);
  writeFileSync(join(root, 'fixture.so'), elfFixture());
  assert.deepEqual(check(), []);
  rmSync(join(root, 'fixture.so'));
  mkdirSync(join(root, 'fixture.so'));
  assert.match(check()[0], /nonempty regular file/);
});

test('Linux bundle input check accepts distro library links and rejects broken links', {
  skip: process.platform === 'win32' ? 'POSIX symlink fixture' : false,
}, (context) => {
  const root = mkdtempSync(join(tmpdir(), 'identity-bundle-inputs-'));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  const config = { bundle: { linux: { appimage: { files: { '/usr/lib/fixture.so': '/fixture.so' } } } } };
  writeFileSync(join(root, 'payload'), elfFixture());
  symlinkSync('payload', join(root, 'fixture.so'));
  assert.deepEqual(linuxBundleInputViolations(config, { sourceRoot: root }), []);
  rmSync(join(root, 'payload'));
  assert.match(linuxBundleInputViolations(config, { sourceRoot: root })[0], /ENOENT/);
});

test('synthetic ELF has a valid section table and truncated tables remain rejected', {
  skip: process.platform !== 'linux' ? 'Linux readelf fixture' : false,
}, (context) => {
  const directory = mkdtempSync(join(tmpdir(), 'identity-elf-'));
  context.after(() => rmSync(directory, { recursive: true, force: true }));
  const path = join(directory, 'fixture.so');
  const bytes = elfFixture();
  writeFileSync(path, bytes);
  const inspect = () => spawnSync('readelf', ['-W', '-h', '-l', '-d', '-V', path], {
    encoding: 'utf8', env: { ...process.env, LC_ALL: 'C' },
  });
  const valid = inspect();
  assert.equal(valid.status, 0, valid.error?.message ?? valid.stderr);
  assert.equal(valid.stderr, '');
  assert.match(valid.stdout, /^\s+LOAD\s/m);
  writeFileSync(path, bytes.subarray(0, bytes.length - 1));
  const invalid = inspect();
  assert.ok(invalid.status !== 0 || invalid.stderr.trim(), 'truncated ELF section table must fail inspection');
});

function writeRuntimeFixture(root) {
  const files = {
    'AppRun': '#!/bin/sh\nsource apprun-hooks/linuxdeploy-plugin-gstreamer.sh\nexec AppRun.wrapped\n',
    'AppRun.wrapped': '#!/bin/sh\n# LD_LIBRARY_PATH=/usr/lib\n',
    'apprun-hooks/linuxdeploy-plugin-gstreamer.sh': [
      'export GST_PLUGIN_SYSTEM_PATH_1_0="${APPDIR}/usr/lib/gstreamer-1.0"',
      'export GST_PLUGIN_PATH_1_0="${APPDIR}/usr/lib/gstreamer-1.0"',
      'export GST_PLUGIN_SCANNER_1_0="${APPDIR}/usr/lib/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner"',
    ].join('\n'),
    'apprun-hooks/linuxdeploy-plugin-gtk.sh': [
      'export GIO_MODULE_DIR="$APPDIR/usr/lib/gio/modules"',
      'export GSETTINGS_SCHEMA_DIR="$APPDIR/usr/share/glib-2.0/schemas"',
      'export GTK_IM_MODULE_FILE="$APPDIR/usr/lib/gtk-3.0/3.0.0/immodules.cache"',
      'export GDK_PIXBUF_MODULE_FILE="$APPDIR/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"',
    ].join('\n'),
    'usr/lib/gio/modules/libgiognutls.so': elfFixture(),
    'usr/lib/gio/modules/giomodule.cache': 'libgiognutls.so: gio-tls-backend\n',
    'usr/lib/gtk-3.0/3.0.0/immodules.cache': '# fixture\n',
    'usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache': '# fixture\n',
    'usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitWebProcess': elfFixture(),
    'usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitNetworkProcess': elfFixture(),
    'usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/injected-bundle/libwebkit2gtkinjectedbundle.so': elfFixture(),
  };
  mkdirSync(join(root, 'usr/share/glib-2.0/schemas'), { recursive: true });
  for (const [file, content] of Object.entries(files)) {
    const path = join(root, file);
    mkdirSync(join(path, '..'), { recursive: true });
    writeFileSync(path, content);
    chmodSync(path, 0o755);
  }
}

test('SquashFS extraction rejects false magic and ambiguous superblocks', () => {
  const bytes = Buffer.alloc(256);
  bytes.write('hsqs', 0);
  bytes.write('hsqs', 100);
  bytes.writeUInt32LE(131072, 112);
  bytes.writeUInt16LE(1, 120);
  bytes.writeUInt16LE(4, 128);
  bytes.writeBigUInt64LE(96n, 140);
  assert.equal(squashfsOffset(bytes), 100);
  const ambiguous = Buffer.concat([bytes, bytes]);
  assert.throws(() => squashfsOffset(ambiguous), /exactly one/);
  assert.throws(() => squashfsOffset(Buffer.from('hsqs')), /exactly one/);
});

test('sidecar provenance rejects version, target, lockfile and source mismatches', () => {
  const expected = { sourceCommit: 'fixture-commit', target: 'x86_64-unknown-linux-gnu',
    cargoLockSha256: 'fixture-hash', applicationVersion: '0.10.7',
    packageName: 'discord-quest-runner', packageVersion: '0.1.0' };
  assert.deepEqual(sidecarProvenanceViolations(expected, expected), []);
  for (const field of Object.keys(expected)) {
    assert.deepEqual(sidecarProvenanceViolations({ ...expected, [field]: 'wrong' }, expected),
      [`sidecar provenance mismatch: ${field}`]);
  }
});

test('sidecar content digest tolerates loader metadata changes and detects code changes', () => {
  const bytes = Buffer.alloc(512);
  elfFixture().copy(bytes);
  bytes.writeBigUInt64LE(256n, 40);
  bytes.writeUInt16LE(4, 60);
  bytes.writeUInt16LE(3, 62);
  bytes.write('code', 128);
  bytes.write('data', 132);
  Buffer.from('\0.text\0.rodata\0.shstrtab\0').copy(bytes, 136);
  for (const [index, name, offset, size] of [[1, 1, 128, 4], [2, 7, 132, 4], [3, 15, 136, 25]]) {
    bytes.writeUInt32LE(name, 256 + index * 64);
    bytes.writeBigUInt64LE(BigInt(offset), 256 + index * 64 + 24);
    bytes.writeBigUInt64LE(BigInt(size), 256 + index * 64 + 32);
  }
  const original = linuxElfContentHash(bytes);
  bytes[32] = 0; // changed program-header location, as a packager may rewrite
  assert.equal(linuxElfContentHash(bytes), original);
  bytes[128] ^= 1;
  assert.notEqual(linuxElfContentHash(bytes), original);
  assert.throws(() => linuxElfContentHash(bytes.subarray(0, 80)), /section table/);
});

test('media file checks accept internal links and reject escaped and broken links', {
  skip: process.platform === 'win32' ? 'POSIX symlink fixture' : false,
}, (context) => {
  const root = mkdtempSync(join(tmpdir(), 'media-links-'));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  writeMediaFixture(root);
  const plugin = join(root, 'usr/lib/gstreamer-1.0/libgstapp.so');
  rmSync(plugin);
  writeFileSync(`${plugin}.1`, elfFixture());
  symlinkSync('libgstapp.so.1', plugin);
  assert.deepEqual(missingAppImageMediaFiles(root), []);
  rmSync(plugin);
  symlinkSync('/etc/passwd', plugin);
  assert.ok(missingAppImageMediaFiles(root).includes('usr/lib/gstreamer-1.0/libgstapp.so'));
  rmSync(plugin);
  symlinkSync('missing.so', plugin);
  assert.ok(missingAppImageMediaFiles(root).includes('usr/lib/gstreamer-1.0/libgstapp.so'));
});

test('AppImage media audit rejects core-only payloads and missing audio factories or scanner', (context) => {
  const root = mkdtempSync(join(tmpdir(), 'appimage-media-'));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, 'usr', 'lib'), { recursive: true });
  writeFileSync(join(root, 'usr', 'lib', 'libgstreamer-1.0.so.0'), 'core library');
  assert.ok(missingAppImageMediaFiles(root).includes('usr/lib/gstreamer-1.0/libgstautodetect.so'));
  writeMediaFixture(root);
  assert.deepEqual(missingAppImageMediaFiles(root), []);
  rmSync(join(root, 'usr/lib/gstreamer-1.0/libgstautodetect.so'));
  writeFileSync(join(root, 'usr/lib/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner'), '');
  assert.deepEqual(missingAppImageMediaFiles(root), [
    'usr/lib/gstreamer-1.0/libgstautodetect.so',
    'usr/lib/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner',
  ]);
});

test('Linux AppDir audit requires desktop integration with the neutral runtime', {
  skip: process.platform !== 'linux' ? 'Linux readelf fixture' : false,
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
  writeFileSync(main, Buffer.concat([elfFixture(), Buffer.from('embedded'), elfFixture()]));
  writeFileSync(bridge, elfFixture());
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
  writeMediaFixture(appDir);
  writeRuntimeFixture(appDir);

  const manifest = auditArtifact({
    platform: 'linux',
    artifact: appDir,
    kind: 'appdir',
    sourceCommit: null,
  });
  assert.equal(manifest.passed, true, JSON.stringify(manifest.violations, null, 2));
  assert.equal(manifest.mainBinary, IDENTITY.mainBinary);
  assert.equal(manifest.bridgeBinary, IDENTITY.bridgeBinary);
  assert.equal(manifest.hashes[IDENTITY.bridgeBinary].length, 64);
  renameSync(main, join(binDir, 'payload'));
  symlinkSync('payload', main);
  const linkedManifest = auditArtifact({ platform: 'linux', sourceCommit: null, artifact: appDir });
  assert.equal(linkedManifest.passed, true, JSON.stringify(linkedManifest.violations, null, 2));
  rmSync(main);
  renameSync(join(binDir, 'payload'), main);

  const scanner = join(appDir, 'usr/lib/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner');
  chmodSync(scanner, 0o644);
  assert.ok(auditArtifact({ platform: 'linux', sourceCommit: null, artifact: appDir }).violations.some((error) => error.includes('not executable')));
  chmodSync(scanner, 0o755);
  const hook = join(appDir, 'apprun-hooks/linuxdeploy-plugin-gstreamer.sh');
  writeFileSync(hook, 'export GST_PLUGIN_PATH_1_0="/usr/lib/gstreamer-1.0"');
  assert.ok(auditArtifact({ platform: 'linux', sourceCommit: null, artifact: appDir }).violations.some((error) => error.includes('incorrect media hook path')));
  writeRuntimeFixture(appDir);
  const wrongArchitecture = elfFixture();
  wrongArchitecture.writeUInt16LE(183, 18);
  writeFileSync(bridge, wrongArchitecture);
  assert.ok(auditArtifact({ platform: 'linux', sourceCommit: null, artifact: appDir }).violations.some((error) => error.includes('wrong ELF architecture')));
  writeFileSync(bridge, elfFixture());

  rmSync(join(appDir, 'usr/lib/gstreamer-1.0/libgstautodetect.so'));
  const missingMedia = auditArtifact({ platform: 'linux', sourceCommit: null, artifact: appDir, kind: 'appdir' });
  assert.equal(missingMedia.passed, false);
  assert.ok(missingMedia.violations.some((violation) => violation.includes('libgstautodetect.so')));
  writeMediaFixture(appDir);
  writeRuntimeFixture(appDir);

  const libraries = join(appDir, 'usr', 'lib');
  mkdirSync(libraries, { recursive: true });
  writeFileSync(join(libraries, 'libwayland-client.so.0'), 'incompatible library');
  const incompatible = auditArtifact({ platform: 'linux', sourceCommit: null, artifact: appDir, kind: 'appdir' });
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
