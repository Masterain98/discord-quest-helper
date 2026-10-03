import { readFileSync, statSync } from 'node:fs';
import { posix, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Check build-system inputs before compilation. sourceRoot also permits static
// verification against an extracted distro package without installing it.
export function linuxBundleInputViolations(config, { sourceRoot = '/' } = {}) {
  const files = config.bundle?.linux?.appimage?.files;
  if (!files || typeof files !== 'object' || Array.isArray(files)) {
    return ['Missing AppImage custom file map'];
  }
  const violations = [];
  for (const [destination, source] of Object.entries(files)) {
    if (typeof source !== 'string' || !posix.isAbsolute(source) || posix.normalize(source) !== source) {
      violations.push(`Invalid AppImage source path: ${destination} <- ${source}`);
      continue;
    }
    const path = resolve(sourceRoot, `.${source}`);
    try {
      // Distribution-provided library symlinks are valid build inputs.
      const stat = statSync(path);
      if (!stat.isFile() || stat.size === 0) throw new Error('not a nonempty regular file');
      if (/\.so(?:\.|$)/.test(posix.basename(destination))) {
        const bytes = readFileSync(path);
        if (bytes.length < 64 || !bytes.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]))
          || bytes[4] !== 2 || bytes[5] !== 1 || bytes.readUInt16LE(18) !== 62) {
          throw new Error('expected x86_64 ELF64 library');
        }
      }
    } catch (error) {
      violations.push(`Invalid AppImage input: ${destination} <- ${source}: ${error.message}`);
    }
  }
  return violations;
}

function main() {
  try {
    const config = JSON.parse(readFileSync(new URL('../src-tauri/tauri.linux.conf.json', import.meta.url), 'utf8'));
    const violations = linuxBundleInputViolations(config);
    if (violations.length) {
      console.error(violations.join('\n'));
      process.exitCode = 1;
    } else {
      console.log('Linux AppImage custom file inputs verified.');
    }
  } catch (error) {
    console.error(`Linux bundle input check failed: ${error.message}`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
