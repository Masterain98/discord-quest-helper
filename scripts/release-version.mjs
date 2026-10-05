// Tauri/Windows package versions omit release-channel suffixes; provenance
// retains the complete public version so different candidates stay distinct.
export function bundleVersion(rawVersion) {
  const version = rawVersion.trim().split('-')[0];
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(`Invalid release version: ${rawVersion}`);
  }
  return version;
}

export function syncCargoLockVersion(lockfile, packageName, version) {
  let matches = 0;
  const updated = lockfile.replace(/\[\[package\]\][\s\S]*?(?=\[\[package\]\]|$)/g, (block) => {
    const name = /^name\s*=\s*"([^"]+)"/m.exec(block)?.[1];
    if (name !== packageName || /^source\s*=/m.test(block)) return block;
    matches += 1;
    if (!/^version\s*=\s*"[^"]+"/m.test(block)) throw new Error(`Missing lockfile version: ${packageName}`);
    return block.replace(/^(version\s*=\s*)"[^"]+"/m, `$1"${version}"`);
  });
  if (matches !== 1) throw new Error(`Expected one workspace lockfile entry for ${packageName}, found ${matches}`);
  return updated;
}
