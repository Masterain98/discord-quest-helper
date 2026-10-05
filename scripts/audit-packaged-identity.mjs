#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import {
  existsSync,
  lstatSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { linuxElfContentHash } from './sidecar-provenance.mjs';
import { bundleVersion } from './release-version.mjs';

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const POLICY = JSON.parse(readFileSync(join(SCRIPT_DIR, 'runtime-identity-tokens.json'), 'utf8'));
const TOKENS = POLICY.tokens;
export const MACOS_SIGNING_ENABLED = false;

export const IDENTITY = Object.freeze({
  publicName: POLICY.identity.publicName,
  mainBinary: POLICY.identity.mainBinary,
  bridgeBinary: POLICY.identity.bridgeBinary,
  runnerBuildBinary: POLICY.identity.runnerBuildBinary,
});

function usage() {
  return `Usage: node scripts/audit-packaged-identity.mjs --platform <linux|macos> --artifact <path> [options]

Options:
  --kind <app|deb|appimage|appdir>  Override artifact detection
  --output <identity-manifest.json> Write the manifest to this path
  --help                            Show this help`;
}

function parseArgs(argv) {
  const result = { platform: null, artifact: null, kind: null, output: null };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    if (flag === '--help' || flag === '-h') return { help: true };
    const value = argv[++index];
    if (!value) throw new Error(`Missing value for ${flag}`);
    if (flag === '--platform') result.platform = value;
    else if (flag === '--artifact') result.artifact = resolve(value);
    else if (flag === '--kind') result.kind = value;
    else if (flag === '--output') result.output = resolve(value);
    else throw new Error(`Unknown option: ${flag}`);
  }
  if (!['linux', 'macos'].includes(result.platform)) throw new Error('--platform must be linux or macos');
  if (!result.artifact || !existsSync(result.artifact)) throw new Error('--artifact must reference an existing path');
  return result;
}

export function containsProductToken(value) {
  const normalized = String(value ?? '').toLowerCase();
  return TOKENS.some((token) => normalized.includes(token.toLowerCase()));
}

export function validateInternalName(name, expected) {
  const validShape = name.length >= 6
    && name.length <= 14
    && /^[a-z]+$/.test(name)
    && !/^[a-f0-9]+$/.test(name);
  return validShape && !containsProductToken(name) && name === expected;
}

function sha256(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

export function pngDimensions(path) {
  try {
    const bytes = readFileSync(path);
    if (bytes.length < 24 || bytes.toString('hex', 0, 8) !== '89504e470d0a1a0a') return null;
    return { width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
  } catch {
    return null;
  }
}

function commandSucceeds(command, args) {
  try {
    execFileSync(command, args, { stdio: ['ignore', 'ignore', 'pipe'] });
    return { ok: true };
  } catch (error) {
    return { ok: false, reason: error?.stderr?.toString().trim() || error.message };
  }
}

function commandOutput(command, args) {
  const result = spawnSync(command, args, { encoding: 'utf8' });
  return {
    ok: result.status === 0,
    output: `${result.stdout || ''}\n${result.stderr || ''}`.trim(),
  };
}

export function parseCodeIdentity(output) {
  const lines = String(output ?? '').split(/\r?\n/);
  const value = (prefix) => {
    const found = lines.find((line) => line.startsWith(prefix))?.slice(prefix.length).trim();
    return found && found !== 'not set' ? found : null;
  };
  return {
    identifier: value('Identifier='),
    teamIdentifier: value('TeamIdentifier='),
    authorities: lines
      .filter((line) => line.startsWith('Authority='))
      .map((line) => line.slice('Authority='.length)),
    hardenedRuntime: lines.some((line) => line.includes('flags=') && line.includes('runtime')),
    adHoc: lines.some((line) => line.trim() === 'Signature=adhoc'),
  };
}

export function relatedCodeIdentityViolations(main, helper) {
  const violations = [];
  if (!main.hardenedRuntime || !helper.hardenedRuntime) {
    violations.push('main app or runtime bridge signature is missing hardened runtime');
  }
  if (!main.adHoc || !helper.adHoc) {
    violations.push('main app and runtime bridge must both use ad-hoc signatures');
  }
  return violations;
}

function plistValue(app, key) {
  try {
    return execFileSync('/usr/libexec/PlistBuddy', [
      '-c', `Print :${key}`, join(app, 'Contents', 'Info.plist'),
    ], { encoding: 'utf8' }).trim();
  } catch {
    return null;
  }
}

function executableFiles(root, includeInternalLinks = false) {
  const files = [];
  const visit = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isSymbolicLink()) {
        if (includeInternalLinks) {
          try {
            const target = realpathSync(path);
            const inside = relative(realpathSync(root), target);
            if (!inside.startsWith('..') && !inside.startsWith('/')
              && statSync(target).isFile() && (statSync(target).mode & 0o111) !== 0) files.push(path);
          } catch { /* The Linux payload audit reports broken/escaped links. */ }
        }
        continue;
      }
      if (entry.isDirectory()) visit(path);
      else if (entry.isFile() && (statSync(path).mode & 0o111) !== 0) files.push(path);
    }
  };
  visit(root);
  return files;
}

function filesWithSuffix(root, suffix) {
  const files = [];
  const visit = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) visit(path);
      else if (entry.isFile() && entry.name.endsWith(suffix)) files.push(path);
    }
  };
  visit(root);
  return files;
}

// AppImages use the host Mesa/EGL stack. Bundling an older Wayland client
// shadows its host counterpart and can abort WebKitWebProcess before rendering.
// Inspect names as well as symlinks; do not follow links outside the AppDir.
export function incompatibleAppImageLibraries(root) {
  const libraries = [];
  const visit = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) visit(path);
      else if (/^libwayland-client\.so(?:\..*)?$/.test(entry.name)) {
        libraries.push(relative(root, path));
      }
    }
  };
  visit(root);
  return libraries.sort();
}

// WebKit creates a GStreamer pipeline even for muted MP4 reward previews.
// The shared libraries alone are insufficient: bundle the element factories,
// scanner and AppRun hook from Tauri's GStreamer linuxdeploy plugin as well.
export const APPIMAGE_MEDIA_FILES = Object.freeze([
    'usr/lib/gstreamer-1.0/libgstapp.so',
    'usr/lib/gstreamer-1.0/libgstautodetect.so',
    'usr/lib/gstreamer-1.0/libgstcoreelements.so',
    'usr/lib/gstreamer-1.0/libgstisomp4.so',
    'usr/lib/gstreamer-1.0/libgstlibav.so',
    'usr/lib/gstreamer-1.0/libgstplayback.so',
    'usr/lib/gstreamer-1.0/libgsttypefindfunctions.so',
    'usr/lib/gstreamer-1.0/libgstvideoparsersbad.so',
    'usr/lib/gstreamer-1.0/libgstvideoconvert.so',
    'usr/lib/gstreamer-1.0/libgstaudioconvert.so',
    'usr/lib/gstreamer-1.0/libgstaudioresample.so',
    'usr/lib/gstreamer-1.0/libgstvolume.so',
    'usr/lib/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner',
    'apprun-hooks/linuxdeploy-plugin-gstreamer.sh',
]);

export function missingAppImageMediaFiles(root) {
  return APPIMAGE_MEDIA_FILES.filter((file) => {
    const path = join(root, file);
    try {
      const target = realpathSync(path);
      const inside = relative(realpathSync(root), target);
      return inside.startsWith('..') || inside.startsWith('/') || !statSync(target).isFile() || statSync(target).size === 0;
    } catch {
      return true;
    }
  });
}

function parseDesktopEntry(path) {
  const fields = {};
  for (const line of readFileSync(path, 'utf8').split(/\r?\n/)) {
    const match = /^([A-Za-z][A-Za-z0-9]*)=(.*)$/.exec(line);
    if (match && fields[match[1]] === undefined) fields[match[1]] = match[2];
  }
  return fields;
}

function auditMacApp(app) {
  if (!app.endsWith('.app')) throw new Error('macOS artifact must be an .app bundle');
  const executableName = plistValue(app, 'CFBundleExecutable');
  const executable = executableName ? join(app, 'Contents', 'MacOS', executableName) : null;
  const nested = executableFiles(join(app, 'Contents'));
  const bridge = nested.find((file) => basename(file) === IDENTITY.bridgeBinary) ?? null;
  const violations = [];

  if (!validateInternalName(executableName ?? '', IDENTITY.mainBinary)) {
    violations.push(`CFBundleExecutable must be ${IDENTITY.mainBinary}`);
  }
  if (plistValue(app, 'CFBundleDisplayName') !== IDENTITY.publicName) {
    violations.push(`CFBundleDisplayName must remain ${IDENTITY.publicName}`);
  }
  if (plistValue(app, 'CFBundleName') !== IDENTITY.publicName) {
    violations.push(`CFBundleName must remain ${IDENTITY.publicName}`);
  }
  if (plistValue(app, 'CFBundleIdentifier') !== POLICY.identity.bundleIdentifier) {
    violations.push('CFBundleIdentifier changed without an approved data/keychain migration');
  }
  if (basename(app) !== `${IDENTITY.publicName}.app`) violations.push('public app bundle name changed');
  if (!executable || !existsSync(executable)) violations.push('main bundle executable is missing');
  if (!bridge) violations.push(`nested runtime bridge ${IDENTITY.bridgeBinary} is missing`);

  const icons = filesWithSuffix(join(app, 'Contents'), '.icns').map((file) => {
    const dimensions = commandOutput('/usr/bin/sips', ['-g', 'pixelWidth', '-g', 'pixelHeight', file]);
    const width = Number(/pixelWidth:\s*(\d+)/.exec(dimensions.output)?.[1] ?? 0);
    const height = Number(/pixelHeight:\s*(\d+)/.exec(dimensions.output)?.[1] ?? 0);
    return { name: basename(file), width, height, size: statSync(file).size };
  });
  if (!icons.some(({ width, height, size }) => width > 1 && height > 1 && size > 1024)) {
    violations.push('macOS public icon is missing, empty, or 1x1');
  }

  let signing = { status: 'disabled' };
  if (MACOS_SIGNING_ENABLED) {
    const strictSigning = commandSucceeds('codesign', ['--verify', '--deep', '--strict', '--verbose=4', app]);
    const signingDetails = commandOutput('codesign', ['-dvvv', app]);
    const mainCodeIdentity = parseCodeIdentity(signingDetails.output);
    const bridgeSigningDetails = bridge ? commandOutput('codesign', ['-dvvv', bridge]) : null;
    const bridgeCodeIdentity = bridgeSigningDetails?.ok
      ? parseCodeIdentity(bridgeSigningDetails.output)
      : null;
    const machOExecutables = nested.filter((file) =>
      commandOutput('/usr/bin/file', ['-b', file]).output.includes('Mach-O'));
    const nestedSigning = machOExecutables.map((file) => ({
      name: basename(file),
      signed: commandSucceeds('codesign', ['--verify', '--strict', '--verbose=4', file]).ok,
    }));
    if (!strictSigning.ok) violations.push('strict code-signing verification failed');
    if (nestedSigning.some(({ signed }) => !signed)) violations.push('nested executable signature verification failed');
    if (bridgeCodeIdentity) {
      violations.push(...relatedCodeIdentityViolations(mainCodeIdentity, bridgeCodeIdentity));
    }
    signing = {
      status: 'enabled',
      strict: strictSigning.ok,
      hardenedRuntime: mainCodeIdentity.hardenedRuntime,
      adHocDistribution: mainCodeIdentity.adHoc,
      authorities: mainCodeIdentity.authorities,
      teamIdentifier: mainCodeIdentity.teamIdentifier,
      bridgeIdentity: bridgeCodeIdentity,
      nested: nestedSigning,
    };
  }

  return {
    platform: 'macos',
    artifact: 'app',
    publicName: IDENTITY.publicName,
    mainBinary: executableName,
    bridgeBinary: bridge ? basename(bridge) : null,
    hashes: {
      ...(executable && existsSync(executable) ? { [IDENTITY.mainBinary]: sha256(executable) } : {}),
      ...(bridge ? { [IDENTITY.bridgeBinary]: sha256(bridge) } : {}),
    },
    icons,
    signing,
    knownResiduals: ['bundle identifier retains the public project identity'],
    violations,
  };
}

function detectLinuxKind(path, requested) {
  if (requested) return requested;
  if (path.endsWith('.deb')) return 'deb';
  if (path.endsWith('.AppImage')) return 'appimage';
  if (lstatSync(path).isDirectory()) return 'appdir';
  throw new Error('Could not determine Linux artifact kind');
}

export function squashfsOffset(bytes) {
  // AppImage runtimes can contain the magic string before the actual superblock.
  // Validate the v4 superblock, compression, block size and filesystem bounds.
  const candidates = [];
  for (let offset = bytes.indexOf('hsqs'); offset >= 0; offset = bytes.indexOf('hsqs', offset + 4)) {
    if (offset + 96 > bytes.length) continue;
    const blockSize = bytes.readUInt32LE(offset + 12);
    const used = bytes.readBigUInt64LE(offset + 40);
    if (bytes.readUInt16LE(offset + 28) === 4 && bytes.readUInt16LE(offset + 30) === 0
      && bytes.readUInt16LE(offset + 20) >= 1 && bytes.readUInt16LE(offset + 20) <= 6
      && blockSize >= 4096 && blockSize <= 1048576 && (blockSize & (blockSize - 1)) === 0
      && used >= 96n && BigInt(offset) + used <= BigInt(bytes.length)) candidates.push(offset);
  }
  if (candidates.length !== 1) throw new Error('Expected exactly one valid SquashFS v4 superblock');
  return candidates[0];
}

function extractLinuxArtifact(path, kind, directory) {
  if (kind === 'appdir') return path;
  if (kind === 'deb') {
    execFileSync('dpkg-deb', ['-x', path, directory], { stdio: 'inherit' });
    return directory;
  }
  if (kind === 'appimage') {
    const root = join(directory, 'squashfs-root');
    execFileSync('unsquashfs', ['-no-progress', '-o', String(squashfsOffset(readFileSync(path))), '-d', root, path], { stdio: ['ignore', 'ignore', 'pipe'] });
    return root;
  }
  throw new Error(`Unsupported Linux artifact kind: ${kind}`);
}

export function sidecarProvenanceViolations(provenance, expected, debVersion) {
  const violations = ['sourceCommit', 'target', 'cargoLockSha256', 'applicationVersion', 'packageName', 'packageVersion']
    .filter((field) => provenance[field] !== expected[field])
    .map((field) => `sidecar provenance mismatch: ${field}`);
  if (debVersion !== undefined && debVersion !== bundleVersion(provenance.applicationVersion)) {
    violations.push('DEB version differs from this build');
  }
  return violations;
}

function auditLinux(path, kind, sourceCommit) {
  const temporary = mkdtempSync(join(tmpdir(), 'identity-package-'));
  try {
    const root = extractLinuxArtifact(path, kind, temporary);
    const executables = executableFiles(root, true);
    const names = executables.map((file) => basename(file));
    const main = executables.find((file) => basename(file) === IDENTITY.mainBinary) ?? null;
    const bridge = executables.find((file) => basename(file) === IDENTITY.bridgeBinary) ?? null;
    const internalTokenFiles = executables
      .map((file) => relative(root, file))
      .filter((file) => containsProductToken(basename(file)));
    const desktopEntries = filesWithSuffix(root, '.desktop').map((file) => ({
      path: relative(root, file),
      fields: parseDesktopEntry(file),
    }));
    const integratedDesktop = desktopEntries.find(({ fields }) =>
      fields.Name === IDENTITY.publicName
      && fields.Exec?.includes(IDENTITY.mainBinary)
      && fields.Icon
      && fields.StartupWMClass === IDENTITY.mainBinary
      && fields.Terminal === 'false');
    const violations = [];
    const control = kind === 'deb' ? Object.fromEntries(['Depends', 'Architecture', 'Version'].map((field) =>
      [field, execFileSync('dpkg-deb', ['-f', path, field], { encoding: 'utf8' }).trim()])) : null;
    const linuxRuntime = JSON.parse(execFileSync('python3', [
      join(SCRIPT_DIR, 'audit-linux-elf.py'), root, kind,
      ...(control ? ['--control', JSON.stringify(control)] : []),
    ], { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 }));
    violations.push(...linuxRuntime.violations);
    const sidecars = [];
    if (sourceCommit) {
      const repository = resolve(SCRIPT_DIR, '..');
      for (const name of [IDENTITY.runnerBuildBinary, IDENTITY.bridgeBinary]) {
        const provenancePath = join(repository, 'build', `${name}-provenance.json`);
        if (!existsSync(provenancePath)) {
          violations.push(`missing build provenance: ${name}`);
          continue;
        }
        const provenance = JSON.parse(readFileSync(provenancePath, 'utf8'));
        const runner = name === IDENTITY.runnerBuildBinary;
        const packageManifest = readFileSync(join(repository, runner ? 'src-runner' : 'src-cdp-launcher', 'Cargo.toml'), 'utf8');
        violations.push(...sidecarProvenanceViolations(provenance, {
          sourceCommit, target: 'x86_64-unknown-linux-gnu',
          cargoLockSha256: sha256(join(repository, 'Cargo.lock')),
          applicationVersion: readFileSync(join(repository, 'public', 'version.txt'), 'utf8').trim(),
          packageName: runner ? 'discord-quest-runner' : 'discord-cdp-launcher',
          packageVersion: /^version\s*=\s*"([^"]+)"/m.exec(packageManifest)?.[1],
        }, control?.Version).map((error) => `${name}: ${error}`));
        if (name === IDENTITY.bridgeBinary) {
          if (!bridge || (kind === 'deb' ? sha256(bridge) !== provenance.sha256
            : linuxElfContentHash(readFileSync(bridge)) !== provenance.elfContentSha256)) {
            violations.push('packaged bridge differs from this build');
          }
          provenance.packagedSha256 = bridge ? sha256(bridge) : null;
          provenance.verification = kind === 'deb' ? 'whole file' : 'ELF allocated sections except dynamic loader tables; loader metadata audited separately';
        } else {
          const runner = readFileSync(join(repository, 'src-tauri', 'data', IDENTITY.runnerBuildBinary));
          if (!runner.length || createHash('sha256').update(runner).digest('hex') !== provenance.sha256
            || !main || readFileSync(main).indexOf(runner) < 0) violations.push('embedded runner differs from this build');
        }
        sidecars.push({ name, ...provenance });
      }
    }
    if (kind === 'appimage' || kind === 'appdir') {
      for (const library of incompatibleAppImageLibraries(root)) {
        violations.push(`AppImage must use the host Wayland client library: ${library}`);
      }
      for (const file of missingAppImageMediaFiles(root)) {
        violations.push(`AppImage media framework is incomplete: missing ${file}`);
      }
    }
    if (!main) violations.push(`Linux payload must contain ${IDENTITY.mainBinary}`);
    if (!bridge) violations.push(`Linux payload must contain ${IDENTITY.bridgeBinary}`);
    if (internalTokenFiles.length) violations.push('executable filenames contain product tokens');
    if (!integratedDesktop) {
      violations.push('desktop entry must preserve the public name/icon and map to the neutral runtime');
    }
    const icons = filesWithSuffix(root, '.png').map((file) => ({
      path: relative(root, file),
      dimensions: pngDimensions(file),
      size: statSync(file).size,
    }));
    if (!icons.some(({ dimensions }) => dimensions && dimensions.width > 1 && dimensions.height > 1)) {
      violations.push('Linux public icon is missing or 1x1');
    }

    return {
      platform: 'linux',
      artifact: kind,
      publicName: IDENTITY.publicName,
      mainBinary: main ? basename(main) : null,
      bridgeBinary: bridge ? basename(bridge) : null,
      hashes: {
        ...(main ? { [IDENTITY.mainBinary]: sha256(main) } : {}),
        ...(bridge ? { [IDENTITY.bridgeBinary]: sha256(bridge) } : {}),
      },
      signing: { status: 'not-applicable' },
      icons,
      executableNames: [...new Set(names)].sort(),
      desktopEntries,
      linuxRuntime,
      evidence: {
        sourceCommit: sourceCommit || null,
        sourceVerification: sourceCommit ? 'CI checkout claim; payload hashes recorded separately' : 'not supplied',
        artifactSha256: kind === 'appdir' ? null : sha256(path),
        debControl: control,
        sidecars,
        sidecarVerification: sourceCommit ? 'checked against this checkout build inputs; see violations' : 'not verified: build inputs unavailable',
        missingDependencies: linuxRuntime.missingDependencies,
        hostDependencies: linuxRuntime.hostDependencies,
      },
      knownResiduals: kind === 'appimage'
        ? ['outer AppImage filename and standard APPIMAGE/APPDIR/ARGV0 variables may retain public identity']
        : [],
      violations,
    };
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
}

export function auditArtifact(options) {
  const kind = options.platform === 'macos' ? 'app' : detectLinuxKind(options.artifact, options.kind);
  const result = options.platform === 'macos'
    ? auditMacApp(options.artifact)
    : auditLinux(options.artifact, kind, auditSourceCommit(options));
  return {
    schemaVersion: 1,
    generatedAt: new Date().toISOString(),
    ...result,
    passed: result.violations.length === 0,
  };
}

export function auditSourceCommit(options) {
  if (options.sourceCommit !== undefined) return options.sourceCommit;
  // Event SHAs may differ from checkout HEAD (PR merge refs or custom refs).
  // Preserve optional provenance checks outside CI and explicit overrides.
  return process.env.GITHUB_SHA
    ? execFileSync('git', ['rev-parse', 'HEAD'], { cwd: resolve(SCRIPT_DIR, '..'), encoding: 'utf8' }).trim()
    : undefined;
}

function main() {
  try {
    const options = parseArgs(process.argv.slice(2));
    if (options.help) {
      console.log(usage());
      return;
    }
    const manifest = auditArtifact(options);
    const json = `${JSON.stringify(manifest, null, 2)}\n`;
    if (options.output) writeFileSync(options.output, json);
    else process.stdout.write(json);
    if (!manifest.passed) process.exitCode = 1;
  } catch (error) {
    console.error(`Packaged identity audit failed: ${error.message}`);
    process.exitCode = 2;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
