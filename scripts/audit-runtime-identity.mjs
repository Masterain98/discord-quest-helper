#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { existsSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const TOKEN_FILE = join(SCRIPT_DIR, 'runtime-identity-tokens.json');
const TOKENS = JSON.parse(readFileSync(TOKEN_FILE, 'utf8')).tokens;

function usage() {
  return `Usage: scripts/audit-runtime-identity.sh [options]

Options:
  --pid <pid>                 Process to inspect (defaults to this audit process)
  --app <path>                macOS .app bundle to inspect
  --desktop-file <path>       Linux .desktop entry to inspect
  --x11-window <id>           X11 window ID to inspect with xprop
  --wayland-log <path>        WAYLAND_DEBUG=client output containing set_app_id
  --fingerprint-file <path>   Local running-games snapshot; only a fingerprint summary is retained
  --output <path>             Write JSON to a file instead of stdout
  --build <debug|release>     Build classification (defaults to unknown)
  --artifact <kind>           Artifact kind such as deb, appimage, app, or development
  --help                      Show this help

The audit never records Authorization, Cookie, account tokens, user IDs, or raw
fingerprint values. Home-directory prefixes are replaced with $HOME.`;
}

function parseArgs(argv) {
  const options = {
    pid: process.pid,
    app: null,
    desktopFile: null,
    x11Window: null,
    waylandLog: null,
    fingerprintFile: null,
    output: null,
    build: 'unknown',
    artifact: 'development',
  };

  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    if (flag === '--help' || flag === '-h') return { help: true };
    const value = argv[index + 1];
    if (!value || value.startsWith('--')) throw new Error(`Missing value for ${flag}`);
    index += 1;
    switch (flag) {
      case '--pid':
        options.pid = Number.parseInt(value, 10);
        if (!Number.isSafeInteger(options.pid) || options.pid <= 0) throw new Error('PID must be a positive integer');
        break;
      case '--app': options.app = resolve(value); break;
      case '--desktop-file': options.desktopFile = resolve(value); break;
      case '--x11-window': options.x11Window = value; break;
      case '--wayland-log': options.waylandLog = resolve(value); break;
      case '--fingerprint-file': options.fingerprintFile = resolve(value); break;
      case '--output': options.output = resolve(value); break;
      case '--build': options.build = value; break;
      case '--artifact': options.artifact = value; break;
      default: throw new Error(`Unknown option: ${flag}`);
    }
  }
  return options;
}

export function containsProductToken(value) {
  if (value === null || value === undefined) return false;
  const normalized = String(value).toLowerCase();
  return TOKENS.some((token) => normalized.includes(token.toLowerCase()));
}

export function redactPath(value, home = process.env.HOME) {
  if (typeof value !== 'string') return value;
  let redacted = value;
  if (home) {
    const normalizedHome = home.endsWith('/') ? home.slice(0, -1) : home;
    redacted = redacted.replaceAll(normalizedHome, '$HOME');
  }
  return redacted;
}

function run(command, args = []) {
  try {
    return {
      status: 'available',
      value: execFileSync(command, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim(),
    };
  } catch (error) {
    return {
      status: 'unavailable',
      reason: error?.stderr?.toString().trim() || error.message,
    };
  }
}

function inspected(value) {
  return {
    value: redactPath(value),
    containsProductToken: containsProductToken(value),
  };
}

function readText(path) {
  try {
    return readFileSync(path, 'utf8').replaceAll('\0', ' ').trim();
  } catch {
    return null;
  }
}

function readLink(path) {
  try {
    return realpathSync(path);
  } catch {
    return null;
  }
}

function processAudit(pid, platform) {
  let executable = null;
  let comm = null;
  let command = null;
  let argv0 = null;
  let tree = { status: 'unavailable', reason: 'process tree collection is unsupported' };

  if (platform === 'linux') {
    executable = readLink(`/proc/${pid}/exe`);
    comm = readText(`/proc/${pid}/comm`);
    const cmdline = readText(`/proc/${pid}/cmdline`);
    argv0 = cmdline?.split(' ')[0] || null;
    command = cmdline;
    tree = run('ps', ['-eo', 'pid=,ppid=,comm=,args=']);
  } else if (platform === 'macos') {
    const commResult = run('ps', ['-p', String(pid), '-o', 'comm=']);
    const commandResult = run('ps', ['-p', String(pid), '-o', 'command=']);
    comm = commResult.value || null;
    command = commandResult.value || null;
    executable = comm;
    // `ps command` does not quote spaces in a macOS bundle path. `comm` is the
    // kernel-reported executable path and therefore the reliable argv[0]
    // identity surface for this audit.
    argv0 = executable;
    tree = run('ps', ['-axo', 'pid=,ppid=,comm=,args=']);
  }

  return {
    pid,
    executablePath: inspected(executable),
    executableBasename: inspected(executable ? basename(executable) : null),
    comm: inspected(comm),
    argv0: inspected(argv0),
    command: inspected(command),
    tree: tree.status === 'available'
      ? { status: 'available', containsProductToken: containsProductToken(tree.value) }
      : { status: 'unavailable', reason: tree.reason },
  };
}

function parseDesktopFile(path) {
  if (!path) return { status: 'not-requested' };
  const content = readText(path);
  if (content === null) return { status: 'unavailable', reason: 'desktop file could not be read' };
  const fields = {};
  for (const line of content.split(/\r?\n/)) {
    const match = /^(Name|Icon|Exec|StartupWMClass|Terminal)=(.*)$/.exec(line);
    if (match && fields[match[1]] === undefined) fields[match[1]] = inspected(match[2]);
  }
  return {
    status: 'available',
    fileName: inspected(basename(path)),
    fields,
  };
}

function linuxDesktopAudit(options) {
  const x11 = options.x11Window
    ? run('xprop', ['-id', options.x11Window, 'WM_CLASS'])
    : { status: 'not-requested' };
  const waylandContent = options.waylandLog ? readText(options.waylandLog) : null;
  const appIds = waylandContent
    ? [...waylandContent.matchAll(/set_app_id[^"\n]*"([^"]+)"/g)].map((match) => inspected(match[1]))
    : [];
  return {
    entry: parseDesktopFile(options.desktopFile),
    x11: x11.status === 'available'
      ? { status: 'available', wmClass: inspected(x11.value) }
      : x11,
    wayland: options.waylandLog
      ? {
          status: waylandContent === null ? 'unavailable' : 'available',
          appIds,
          containsProductToken: containsProductToken(waylandContent),
        }
      : { status: 'not-requested' },
  };
}

function plistValue(app, key) {
  const plist = join(app, 'Contents', 'Info.plist');
  const result = run('/usr/libexec/PlistBuddy', ['-c', `Print :${key}`, plist]);
  return result.status === 'available' ? result.value : null;
}

function macBundleAudit(app) {
  if (!app) return { status: 'not-requested' };
  if (!existsSync(app)) return { status: 'unavailable', reason: 'app bundle does not exist' };

  const executableName = plistValue(app, 'CFBundleExecutable');
  const executablePath = executableName ? join(app, 'Contents', 'MacOS', executableName) : null;
  const display = run('codesign', ['-dvvv', app]);
  const verify = run('codesign', ['--verify', '--deep', '--strict', '--verbose=4', app]);
  const assess = run('spctl', ['--assess', '--type', 'execute', '--verbose=4', app]);
  const nested = run('find', [join(app, 'Contents'), '-type', 'f', '-perm', '+111']);

  return {
    status: 'available',
    appName: inspected(basename(app)),
    infoPlist: {
      CFBundleExecutable: inspected(executableName),
      CFBundleDisplayName: inspected(plistValue(app, 'CFBundleDisplayName')),
      CFBundleName: inspected(plistValue(app, 'CFBundleName')),
      CFBundleIdentifier: inspected(plistValue(app, 'CFBundleIdentifier')),
    },
    executablePath: inspected(executablePath),
    signing: {
      display: display.status === 'available'
        ? { status: 'available', containsProductToken: containsProductToken(display.value) }
        : { status: 'unavailable', reason: display.reason },
      strictVerification: { status: verify.status, reason: redactPath(verify.reason) || null },
      gatekeeperAssessment: { status: assess.status, reason: redactPath(assess.reason) || null },
    },
    nestedExecutables: nested.status === 'available'
      ? nested.value.split(/\r?\n/).filter(Boolean).map((path) => inspected(path))
      : [],
  };
}

function findFingerprint(value, key = '') {
  if (key.toLowerCase().includes('fingerprint') && ['string', 'number', 'bigint'].includes(typeof value)) {
    return String(value);
  }
  if (Array.isArray(value)) {
    for (const item of value) {
      const found = findFingerprint(item);
      if (found !== null) return found;
    }
  } else if (value && typeof value === 'object') {
    for (const [childKey, child] of Object.entries(value)) {
      const found = findFingerprint(child, childKey);
      if (found !== null) return found;
    }
  }
  return null;
}

export function fingerprintSummary(path) {
  if (!path) return { status: 'unavailable', reason: 'no fingerprint snapshot was provided' };
  try {
    const parsed = JSON.parse(readFileSync(path, 'utf8'));
    const fingerprint = findFingerprint(parsed);
    if (fingerprint === null) return { status: 'unavailable', reason: 'no native executable fingerprint was present' };
    if (fingerprint === '<unavailable>' || fingerprint === '<undefined>') {
      return { status: 'unavailable', reason: 'Discord native fingerprint probe returned unavailable' };
    }
    return {
      status: 'available',
      length: Buffer.byteLength(fingerprint, 'utf8'),
      sha256: createHash('sha256').update(fingerprint).digest('hex'),
      containsProductToken: containsProductToken(fingerprint),
    };
  } catch (error) {
    return { status: 'error', reason: `fingerprint snapshot could not be parsed: ${error.message}` };
  }
}

function linuxEnvironmentAudit() {
  const fields = {};
  for (const name of ['APPIMAGE', 'APPDIR', 'ARGV0']) {
    const value = process.env[name];
    fields[name] = {
      present: value !== undefined,
      containsProductToken: containsProductToken(value),
    };
  }
  return fields;
}

function collectAudit(options) {
  const platform = process.platform === 'darwin' ? 'macos' : process.platform;
  if (!['linux', 'macos'].includes(platform)) throw new Error('Only Linux and macOS are supported');
  return {
    schemaVersion: 1,
    generatedAt: new Date().toISOString(),
    platform,
    build: options.build,
    artifact: options.artifact,
    process: processAudit(options.pid, platform),
    desktop: platform === 'linux' ? linuxDesktopAudit(options) : { status: 'not-applicable' },
    appImageEnvironment: platform === 'linux' ? linuxEnvironmentAudit() : null,
    macosBundle: platform === 'macos' ? macBundleAudit(options.app) : { status: 'not-applicable' },
    nativeExecutableFingerprint: fingerprintSummary(options.fingerprintFile),
    privacy: {
      homePathsRedacted: true,
      rawFingerprintStored: false,
      sensitiveHeadersStored: false,
    },
  };
}

function main() {
  try {
    const options = parseArgs(process.argv.slice(2));
    if (options.help) {
      console.log(usage());
      return;
    }
    const output = `${JSON.stringify(collectAudit(options), null, 2)}\n`;
    if (options.output) writeFileSync(options.output, output, { mode: 0o600 });
    else process.stdout.write(output);
  } catch (error) {
    console.error(`Runtime identity audit failed: ${error.message}`);
    process.exitCode = 2;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
