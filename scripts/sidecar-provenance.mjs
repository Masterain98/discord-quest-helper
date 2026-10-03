import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

export function linuxElfContentHash(bytes) {
  if (bytes.length < 64 || bytes[4] !== 2 || bytes[5] !== 1 || bytes.readUInt16LE(18) !== 62) {
    throw new Error('Expected x86_64 ELF64 content');
  }
  const table = Number(bytes.readBigUInt64LE(40));
  const stride = bytes.readUInt16LE(58);
  const count = bytes.readUInt16LE(60);
  const namesIndex = bytes.readUInt16LE(62);
  if (stride !== 64 || namesIndex >= count || table + stride * count > bytes.length) throw new Error('Invalid ELF section table');
  const section = (index) => {
    const offset = table + index * stride;
    const start = Number(bytes.readBigUInt64LE(offset + 24));
    const size = Number(bytes.readBigUInt64LE(offset + 32));
    if (!Number.isSafeInteger(start) || !Number.isSafeInteger(size) || start + size > bytes.length) throw new Error('Invalid ELF section bounds');
    return { name: bytes.readUInt32LE(offset), bytes: bytes.subarray(start, start + size) };
  };
  const names = section(namesIndex).bytes;
  const selected = new Map();
  for (let index = 0; index < count; index += 1) {
    const offset = table + index * stride;
    const nameIndex = bytes.readUInt32LE(offset);
    const end = names.indexOf(0, nameIndex);
    if (end < 0) throw new Error('Invalid ELF section name');
    const name = names.toString('utf8', nameIndex, end);
    if (name === '.text' || name === '.rodata') selected.set(name, section(index).bytes);
  }
  if (!selected.get('.text')?.length || !selected.get('.rodata')?.length) throw new Error('Missing ELF code or read-only data');
  const hash = createHash('sha256');
  for (const name of ['.text', '.rodata']) hash.update(`${name}:${selected.get(name).length}:`).update(selected.get(name));
  return hash.digest('hex');
}

export function recordSidecar(root, name, path, target, metadata, packageName) {
  const bytes = readFileSync(path);
  if (!bytes.length) throw new Error(`${name} sidecar is empty`);
  if (target === 'x86_64-unknown-linux-gnu'
    && !(bytes.length >= 64 && bytes.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]))
      && bytes[4] === 2 && bytes[5] === 1 && bytes.readUInt16LE(18) === 62)) {
    throw new Error(`${name} sidecar has the wrong Linux ELF architecture`);
  }
  const hash = (data) => createHash('sha256').update(data).digest('hex');
  mkdirSync(join(root, 'build'), { recursive: true });
  writeFileSync(join(root, 'build', `${name}-provenance.json`), `${JSON.stringify({
    sourceCommit: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(),
    target, packageName, packageVersion: metadata.packages.find((pkg) => pkg.name === packageName).version,
    applicationVersion: readFileSync(join(root, 'public', 'version.txt'), 'utf8').trim(),
    cargoLockSha256: hash(readFileSync(join(root, 'Cargo.lock'))),
    sha256: hash(bytes), size: bytes.length,
    ...(target === 'x86_64-unknown-linux-gnu' ? { elfContentSha256: linuxElfContentHash(bytes) } : {}),
  }, null, 2)}\n`);
}
