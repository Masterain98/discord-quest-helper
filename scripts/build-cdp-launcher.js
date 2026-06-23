import { execFileSync } from 'child_process';
import { copyFileSync, existsSync, mkdirSync, statSync, unlinkSync, writeFileSync } from 'fs';
import { dirname, join, resolve } from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const rootDir = resolve(__dirname, '..');
const tauriDir = join(rootDir, 'src-tauri');
const launcherDir = join(rootDir, 'src-cdp-launcher');
const binariesDir = join(tauriDir, 'binaries');

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
const exeName = `discord-cdp-launcher${exeExt}`;
const targetArgs = process.env.CARGO_BUILD_TARGET || process.env.TAURI_TARGET_TRIPLE
  ? ['--target', targetTriple]
  : [];

mkdirSync(binariesDir, { recursive: true });

// Remove stale legacy binaries that could shadow the correct sidecar
// in find_bundled_cdp_launcher()'s search path.
const stalePatterns = [
  'discord-cdp-launcher.exe',
  'discord-cdp-launcher-sidecar.exe',
  'discord-cdp-launcher',
  'discord-cdp-launcher-sidecar',
];
for (const target of ['release', 'debug']) {
  const targetDir = join(tauriDir, 'target', target);
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

const destExe = join(binariesDir, `discord-cdp-launcher-sidecar-${targetTriple}${exeExt}`);
if (!existsSync(destExe)) {
  writeFileSync(destExe, '');
}

console.log(`Building discord-cdp-launcher for ${targetTriple}...`);

execFileSync('cargo', ['build', '--release', ...targetArgs], {
  cwd: launcherDir,
  stdio: 'inherit',
});

const targetDir = targetArgs.length > 0
  ? join(launcherDir, 'target', targetTriple, 'release')
  : join(launcherDir, 'target', 'release');

const sourceExe = join(targetDir, exeName);
if (!existsSync(sourceExe)) {
  throw new Error(`Expected launcher binary was not built: ${sourceExe}`);
}

copyFileSync(sourceExe, destExe);

const size = statSync(destExe).size;
console.log(`Copied launcher to ${destExe} (${size} bytes).`);
