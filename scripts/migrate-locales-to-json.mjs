/**
 * One-time migration script: converts src/locales/*.ts to src/locales/*.json
 *
 * Usage:
 *   pnpm tsx scripts/migrate-locales-to-json.mjs
 *
 * Requires `tsx` as a dev dependency (pnpm add -D tsx).
 */

import fs from 'node:fs/promises'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

const root = process.cwd()
const localesDir = path.join(root, 'src', 'locales')

const entries = await fs.readdir(localesDir)
const tsFiles = entries
  .filter((name) => name.endsWith('.ts') && name !== 'meta.ts')
  .sort()

if (tsFiles.length === 0) {
  console.log('No .ts locale files found.')
  process.exit(0)
}

console.log(`Found ${tsFiles.length} .ts locale files to convert:\n`)

for (const fileName of tsFiles) {
  const inputPath = path.join(localesDir, fileName)
  const outputPath = path.join(localesDir, fileName.replace(/\.ts$/, '.json'))

  const moduleUrl = pathToFileURL(inputPath).href
  const mod = await import(moduleUrl)
  const messages = mod.default

  if (!messages || typeof messages !== 'object' || Array.isArray(messages)) {
    throw new Error(`${fileName} does not export a locale object as default`)
  }

  await fs.writeFile(
    outputPath,
    `${JSON.stringify(messages, null, 2)}\n`,
    'utf8',
  )

  console.log(`  ${fileName} -> ${path.basename(outputPath)}`)
}

console.log(`\nDone. Converted ${tsFiles.length} files.`)
console.log('Verify with: node -e "for (const f of require(\'fs\').readdirSync(\'src/locales\').filter(f=>f.endsWith(\'.json\'))) JSON.parse(require(\'fs\').readFileSync(\'src/locales/\'+f,\'utf8\')); console.log(\'All JSON valid.\')"')
