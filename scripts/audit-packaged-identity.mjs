#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
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
const TOKENS = JSON.parse(readFileSync(join(SCRIPT_DIR, 'runtime-identity-tokens.json'), 'utf8')).tokens;

export const IDENTITY = Object.freeze({
  publicName: 'Discord Quest Helper',
  mainBinary: 'meridian',
  bridgeBinary: 'waybridge',
  runnerBuildBinary: 'stagecraft',
});

function usage() {
  return `Usage: node scripts/audit-packaged-identity.mjs --platform <linux|macos> --artifact <path> [options]

Options:
  --kind <app|deb|appimage|appdir>  Override artifact detection
  --output <identity-manifest.json> Write the manifest to this path
  --allow-unsigned                  Permit unsigned macOS smoke artifacts
  --help                            Show this help`;
}

function parseArgs(argv) {
  const result = { platform: null, artifact: null, kind: null, output: null, allowUnsigned: false };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    if (flag === '--help' || flag === '-h') return { help: true };
    if (flag === '--allow-unsigned') {
      result.allowUnsigned = true;
      continue;
    }
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

function commandSucceeds(command, args) {
  try {
    execFileSync(command, args, { stdio: ['ignore', 'ignore', 'pipe'] });
    return { ok: true };
  } catch (error) {
    return { ok: false, reason: error?.stderr?.toString().trim() || error.message };
  }
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

function auditMacApp(app, allowUnsigned) {
  if (!app.endsWith('.app')) throw new Error('macOS artifact must be an .app bundle');
  const executableName = plistValue(app, 'CFBundleExecutable');
  const executable = executableName ? join(app, 'Contents', 'MacOS', executableName) : null;
  const nested = executableFiles(join(app, 'Contents'));
  const violations = [];

  if (!validateInternalName(executableName ?? '', IDENTITY.mainBinary)) {
    violations.push(`CFBundleExecutable must be ${IDENTITY.mainBinary}`);
  }
  if (plistValue(app, 'CFBundleDisplayName') !== IDENTITY.publicName) {
    violations.push(`CFBundleDisplayName must remain ${IDENTITY.publicName}`);
  }
  if (!executable || !existsSync(executable)) violations.push('main bundle executable is missing');

  const strictSigning = commandSucceeds('codesign', ['--verify', '--deep', '--strict', '--verbose=4', app]);
  if (!allowUnsigned && !strictSigning.ok) violations.push('strict code-signing verification failed');

  return {
    platform: 'macos',
    artifact: 'app',
    publicName: IDENTITY.publicName,
    mainBinary: executableName,
    bridgeBinary: nested.map((file) => basename(file)).find((name) => name === IDENTITY.bridgeBinary) ?? null,
    hashes: executable && existsSync(executable) ? { [IDENTITY.mainBinary]: sha256(executable) } : {},
    signing: { strict: strictSigning.ok, smokeArtifact: allowUnsigned },
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
    const violations = [];
    if (!main) violations.push(`Linux payload must contain ${IDENTITY.mainBinary}`);
    if (internalTokenFiles.length) violations.push('executable filenames contain product tokens');

    return {
      platform: 'linux',
      artifact: kind,
      publicName: IDENTITY.publicName,
      mainBinary: main ? basename(main) : null,
      bridgeBinary: bridge ? basename(bridge) : null,
      hashes: main ? { [IDENTITY.mainBinary]: sha256(main) } : {},
      executableNames: [...new Set(names)].sort(),
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
    ? auditMacApp(options.artifact, options.allowUnsigned)
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
