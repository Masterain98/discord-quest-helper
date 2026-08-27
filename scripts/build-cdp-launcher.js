import { execFileSync } from 'child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, statSync, unlinkSync, writeFileSync } from 'fs';
import { dirname, join, resolve } from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const rootDir = resolve(__dirname, '..');
const tauriDir = join(rootDir, 'src-tauri');
const binariesDir = join(tauriDir, 'binaries');
const manifestPath = join(rootDir, 'Cargo.toml');
const runtimePolicy = JSON.parse(readFileSync(join(rootDir, 'scripts', 'runtime-identity-tokens.json'), 'utf8'));
if (runtimePolicy.policies.macosSigningEnabled !== false) {
  throw new Error('macOS signing policy must remain disabled.');
}
const macosSigningEnabled = false;
const runtimeBridgeName = runtimePolicy.identity.bridgeBinary;

function rustHostTriple() {
  const output = execFileSync('rustc', ['-vV'], { encoding: 'utf8' });
  const hostLine = output.split(/\r?\n/).find(line => line.startsWith('host:'));
  if (!hostLine) {
    throw new Error('Could not determine rust host triple from `rustc -vV`.');
  }
  return hostLine.replace('host:', '').trim();
}

const targetTriple = process.env.CARGO_BUILD_TARGET || process.env.TAURI_TARGET_TRIPLE || rustHostTriple();
const isWindowsTarget = targetTriple.includes('windows');
const exeExt = isWindowsTarget ? '.exe' : '';
const exeName = `${runtimeBridgeName}${exeExt}`;
const metadata = JSON.parse(execFileSync(
  'cargo',
  [
    'metadata',
    '--format-version', '1',
    '--no-deps',
    '--manifest-path', manifestPath,
  ],
  { cwd: rootDir, encoding: 'utf8' },
));
const cargoTargetDir = metadata.target_directory;

mkdirSync(binariesDir, { recursive: true });

// Remove stale legacy binaries that could shadow the correct sidecar
// in find_bundled_cdp_launcher()'s search path.
const stalePatterns = [
  'discord-cdp-launcher.exe',
  'discord-cdp-launcher-sidecar.exe',
  'discord-cdp-launcher',
  'discord-cdp-launcher-sidecar',
  'waybridge.exe',
  'waybridge',
];
for (const baseTargetDir of [cargoTargetDir, join(tauriDir, 'target')]) {
  for (const target of ['release', 'debug']) {
    const targetDir = join(baseTargetDir, target);
    for (const name of stalePatterns) {
      const stale = join(targetDir, name);
      if (existsSync(stale)) {
        try {
          unlinkSync(stale);
          console.log(`Removed stale legacy binary: ${stale}`);
        } catch (error) {
          console.warn(`Could not remove stale legacy binary ${stale}: ${error instanceof Error ? error.message : String(error)}`);
        }
      }
    }
  }
}

const destExe = join(binariesDir, `${runtimeBridgeName}-${targetTriple}${exeExt}`);
if (!existsSync(destExe)) {
  writeFileSync(destExe, '');
}

console.log(`Building runtime bridge for ${targetTriple}...`);

execFileSync('cargo', [
  'build',
  '--manifest-path', manifestPath,
  '--package', 'discord-cdp-launcher',
  '--profile', 'sidecar-release',
  '--target', targetTriple,
], {
  cwd: rootDir,
  stdio: 'inherit',
});

const sourceExe = join(cargoTargetDir, targetTriple, 'sidecar-release', exeName);
if (!existsSync(sourceExe)) {
  throw new Error(`Expected launcher binary was not built: ${sourceExe}`);
}

copyFileSync(sourceExe, destExe);

if (targetTriple.includes('apple-darwin') && macosSigningEnabled) {
  execFileSync('/usr/bin/codesign', ['--force', '--sign', '-', destExe], { stdio: 'inherit' });
  execFileSync('/usr/bin/codesign', ['--verify', '--strict', '--verbose=2', destExe], { stdio: 'inherit' });
} else if (targetTriple.includes('apple-darwin')) {
  console.log('macOS signing is disabled by repository policy.');
}

const size = statSync(destExe).size;
console.log(`Copied launcher to ${destExe} (${size} bytes).`);
