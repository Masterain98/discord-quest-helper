import { execFileSync } from 'child_process';
import { copyFileSync, existsSync, mkdirSync, statSync, unlinkSync, writeFileSync } from 'fs';
import { dirname, join, resolve } from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const rootDir = resolve(__dirname, '..');
const tauriDir = join(rootDir, 'src-tauri');
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
const destExe = join(binariesDir, `discord-cdp-launcher-sidecar-${targetTriple}${exeExt}`);
if (!existsSync(destExe)) {
  writeFileSync(destExe, '');
}

console.log(`Building discord-cdp-launcher for ${targetTriple}...`);

execFileSync('cargo', ['build', '--release', '--bin', 'discord-cdp-launcher', ...targetArgs], {
  cwd: tauriDir,
  stdio: 'inherit',
});

const targetDir = targetArgs.length > 0
  ? join(tauriDir, 'target', targetTriple, 'release')
  : join(tauriDir, 'target', 'release');

const sourceExe = join(targetDir, exeName);
if (!existsSync(sourceExe)) {
  throw new Error(`Expected launcher binary was not built: ${sourceExe}`);
}

copyFileSync(sourceExe, destExe);

const size = statSync(destExe).size;
console.log(`Copied launcher to ${destExe} (${size} bytes).`);

try {
  unlinkSync(sourceExe);
  console.log(`Removed temporary build output ${sourceExe}.`);
} catch (error) {
  console.warn(`Could not remove temporary launcher output: ${error.message}`);
}
