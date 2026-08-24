import { execFileSync } from 'child_process';
import { copyFileSync, writeFileSync, mkdirSync, existsSync } from 'fs';
import { join, resolve } from 'path';
import { fileURLToPath } from 'url';
import { dirname } from 'path';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const rootDir = resolve(__dirname, '..');
const tauriDataDir = join(rootDir, 'src-tauri', 'data');
const manifestPath = join(rootDir, 'Cargo.toml');

function rustHostTriple() {
    const output = execFileSync('rustc', ['-vV'], { encoding: 'utf8' });
    const hostLine = output.split(/\r?\n/).find(line => line.startsWith('host:'));
    if (!hostLine) {
        throw new Error('Could not determine rust host triple from `rustc -vV`.');
    }
    return hostLine.replace('host:', '').trim();
}

const targetTriple = process.env.CARGO_BUILD_TARGET || process.env.TAURI_TARGET_TRIPLE || rustHostTriple();
const ext = targetTriple.includes('windows') ? '.exe' : '';
const exeName = `stagecraft${ext}`;
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
const runnerTargetDir = join(metadata.target_directory, targetTriple, 'sidecar-release');

const sourceExe = join(runnerTargetDir, exeName);
const destExe = join(tauriDataDir, exeName);

console.log('🚀 Building simulated-game runtime...');

try {
    execFileSync('cargo', [
        'build',
        '--manifest-path', manifestPath,
        '--package', 'discord-quest-runner',
        '--profile', 'sidecar-release',
        '--target', targetTriple,
    ], {
        cwd: rootDir,
        stdio: 'inherit'
    });
    console.log('✅ Build successful.');

    if (!existsSync(tauriDataDir)) {
        mkdirSync(tauriDataDir, { recursive: true });
    }

    console.log(`📦 Copying ${exeName} to src-tauri/data/...`);
    copyFileSync(sourceExe, destExe);
    if (targetTriple.includes('apple-darwin')) {
        execFileSync('/usr/bin/codesign', ['--force', '--sign', '-', destExe], { stdio: 'inherit' });
        execFileSync('/usr/bin/codesign', ['--verify', '--strict', '--verbose=2', destExe], { stdio: 'inherit' });
    }
    console.log('✨ Runner copied successfully.');

    // Write runner version info (git hash + build timestamp)
    let commitHash = 'unknown';
    try {
        commitHash = execFileSync('git', ['rev-parse', '--short', 'HEAD'], {
            cwd: rootDir,
            encoding: 'utf-8'
        }).trim();
    } catch {
        console.warn('⚠️  Could not get git commit hash');
    }
    const buildTime = new Date().toISOString();
    const versionInfo = `${commitHash}\n${buildTime}\n`;
    const versionFile = join(tauriDataDir, 'runner-version.txt');
    writeFileSync(versionFile, versionInfo);
    console.log(`📋 Runner version info written: ${commitHash} @ ${buildTime}`);

} catch (error) {
    console.error('❌ Failed to build or copy runner:', error.message);
    process.exit(1);
}
