import { execSync } from 'child_process';
import { copyFileSync, mkdirSync, existsSync } from 'fs';
import { join, resolve } from 'path';
import { fileURLToPath } from 'url';
import { dirname } from 'path';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const rootDir = resolve(__dirname, '..');
const runnerDir = join(rootDir, 'src-runner');
const tauriDataDir = join(rootDir, 'src-tauri', 'data');
const runnerTargetDir = join(runnerDir, 'target', 'release');

const platform = process.platform;
const ext = platform === 'win32' ? '.exe' : '';
const exeName = `discord-quest-runner${ext}`;

const sourceExe = join(runnerTargetDir, exeName);
const destExe = join(tauriDataDir, exeName);

console.log('🚀 Building discord-quest-runner...');

try {
    execSync('cargo build --release', {
        cwd: runnerDir,
        stdio: 'inherit'
    });
    console.log('✅ Build successful.');

    if (!existsSync(tauriDataDir)) {
        mkdirSync(tauriDataDir, { recursive: true });
    }

    console.log(`📦 Copying ${exeName} to src-tauri/data/...`);
    copyFileSync(sourceExe, destExe);
    console.log('✨ Runner copied successfully.');

} catch (error) {
    console.error('❌ Failed to build or copy runner:', error.message);
    process.exit(1);
}
