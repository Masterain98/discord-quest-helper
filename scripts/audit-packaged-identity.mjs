#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import {
  existsSync,
  lstatSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

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

function executableFiles(root) {
  const files = [];
  const visit = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isSymbolicLink()) continue;
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

function extractLinuxArtifact(path, kind, directory) {
  if (kind === 'appdir') return path;
  if (kind === 'deb') {
    execFileSync('dpkg-deb', ['-x', path, directory], { stdio: 'inherit' });
    return directory;
  }
  if (kind === 'appimage') {
    execFileSync(path, ['--appimage-extract'], { cwd: directory, stdio: 'inherit' });
    return join(directory, 'squashfs-root');
  }
  throw new Error(`Unsupported Linux artifact kind: ${kind}`);
}

function auditLinux(path, kind) {
  const temporary = mkdtempSync(join(tmpdir(), 'identity-package-'));
  try {
    const root = extractLinuxArtifact(path, kind, temporary);
    const executables = executableFiles(root);
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
    : auditLinux(options.artifact, kind);
  return {
    schemaVersion: 1,
    generatedAt: new Date().toISOString(),
    ...result,
    passed: result.violations.length === 0,
  };
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
