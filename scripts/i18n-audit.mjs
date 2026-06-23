import fs from 'node:fs'
import path from 'node:path'
import ts from 'typescript'

const ROOT = process.cwd()
const EN_FILE = path.join(ROOT, 'src/locales/en.ts')
const REPORT_DIR = path.join(ROOT, 'reports')

const SOURCE_EXTS = new Set(['.ts', '.tsx', '.js', '.jsx', '.vue', '.rs', '.mjs', '.cjs'])
const EXCLUDED_DIRS = new Set(['node_modules', 'dist', 'target', '.git'])

function isExcluded(file) {
  const normalized = file.split(path.sep)
  if (file.endsWith('pnpm-lock.yaml')) return true
  if (normalized.includes('src') && normalized.includes('locales')) return true
  if (normalized.includes('src-tauri') && normalized.includes('target')) return true
  return normalized.some(part => EXCLUDED_DIRS.has(part))
}

function walk(dir, files = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name)
    if (isExcluded(full)) continue

    if (entry.isDirectory()) {
      walk(full, files)
    } else if (SOURCE_EXTS.has(path.extname(full))) {
      files.push(full)
    }
  }
  return files
}

function findDefaultExportObject(sourceFile) {
  let result = null

  function visit(node) {
    if (ts.isExportAssignment(node) && ts.isObjectLiteralExpression(node.expression)) {
      result = node.expression
    }
    ts.forEachChild(node, visit)
  }

  visit(sourceFile)
  return result
}

function propNameToString(name) {
  if (ts.isIdentifier(name)) return name.text
  if (ts.isStringLiteral(name) || ts.isNumericLiteral(name)) return name.text
  return null
}

function flattenLocaleObject(objectLiteral, prefix = [], out = new Map()) {
  for (const prop of objectLiteral.properties) {
    if (!ts.isPropertyAssignment(prop)) continue

    const name = propNameToString(prop.name)
    if (!name) continue

    const next = [...prefix, name]
    const init = prop.initializer

    if (ts.isObjectLiteralExpression(init)) {
      flattenLocaleObject(init, next, out)
    } else if (ts.isStringLiteral(init) || ts.isNoSubstitutionTemplateLiteral(init)) {
      const key = next.join('.')
      out.set(key, {
        key,
        value: init.text,
        line: ts.getLineAndCharacterOfPosition(init.getSourceFile(), prop.getStart()).line + 1,
      })
    }
  }

  return out
}

function extractEnglishKeys() {
  const text = fs.readFileSync(EN_FILE, 'utf8')
  const sourceFile = ts.createSourceFile(EN_FILE, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS)
  const obj = findDefaultExportObject(sourceFile)

  if (!obj) {
    throw new Error('Cannot find default export object in src/locales/en.ts')
  }

  return flattenLocaleObject(obj)
}

function collectSourceText(files) {
  return files.map(file => ({
    file,
    rel: path.relative(ROOT, file),
    text: fs.readFileSync(file, 'utf8'),
  }))
}

function detectDynamicPrefixes(sources) {
  const prefixes = new Set()
  const manualReview = []
  const unknownDynamic = []

  const callPrefix = String.raw`(?<![\w$])(?:\$?t|i18n\.global\.t)`
  const templatePrefixRegex = new RegExp(`${callPrefix}\\s*\\(\\s*\`([a-zA-Z0-9_.-]+)\\.\\$\\{`, 'g')
  const concatPrefixRegex = new RegExp(`${callPrefix}\\s*\\(\\s*['"]([a-zA-Z0-9_.-]+\\.)['"]\\s*\\+`, 'g')
  const unresolvedTemplateRegex = new RegExp(`${callPrefix}\\s*\\(\\s*\`[^\`]*\\$\\{`, 'g')
  const unresolvedConcatRegex = new RegExp(`${callPrefix}\\s*\\(\\s*[^'"\`)][^)]*\\+`, 'g')

  for (const src of sources) {
    let match

    while ((match = templatePrefixRegex.exec(src.text))) {
      prefixes.add(match[1])
      manualReview.push({
        file: src.rel,
        kind: 'template-prefix',
        prefix: match[1],
        sample: match[0],
      })
    }

    while ((match = concatPrefixRegex.exec(src.text))) {
      const prefix = match[1].replace(/\.$/, '')
      prefixes.add(prefix)
      manualReview.push({
        file: src.rel,
        kind: 'concat-prefix',
        prefix,
        sample: match[0],
      })
    }

    while ((match = unresolvedTemplateRegex.exec(src.text))) {
      if (!match[0].match(/`[a-zA-Z0-9_.-]+\.\$\{/)) {
        unknownDynamic.push({
          file: src.rel,
          kind: 'unknown-template',
          sample: match[0],
        })
      }
    }

    while ((match = unresolvedConcatRegex.exec(src.text))) {
      unknownDynamic.push({
        file: src.rel,
        kind: 'unknown-concat',
        sample: match[0],
      })
    }
  }

  return { prefixes: [...prefixes].sort(), manualReview, unknownDynamic }
}

function main() {
  fs.mkdirSync(REPORT_DIR, { recursive: true })

  const keys = extractEnglishKeys()
  const sourceFiles = walk(ROOT)
  const sources = collectSourceText(sourceFiles)
  const combined = sources.map(s => s.text).join('\n')
  const { prefixes, manualReview, unknownDynamic } = detectDynamicPrefixes(sources)

  const rows = []

  for (const [key, meta] of keys) {
    const literalUsed = combined.includes(key)
    const protectedByPrefix = prefixes.find(prefix => key === prefix || key.startsWith(`${prefix}.`))

    rows.push({
      key,
      value: meta.value,
      line: meta.line,
      status: literalUsed
        ? 'used'
        : protectedByPrefix
          ? 'protected-dynamic-prefix'
          : 'unused-candidate',
      protectedByPrefix: protectedByPrefix ?? null,
    })
  }

  const unused = rows.filter(row => row.status === 'unused-candidate')
  const protectedRows = rows.filter(row => row.status === 'protected-dynamic-prefix')

  fs.writeFileSync(
    path.join(REPORT_DIR, 'i18n-unused-keys.json'),
    JSON.stringify({ unused, protected: protectedRows, manualReview, unknownDynamic }, null, 2),
  )

  fs.writeFileSync(
    path.join(REPORT_DIR, 'i18n-unused-keys.md'),
    [
      '# i18n unused key audit',
      '',
      `Total English keys: ${rows.length}`,
      `Unused candidates: ${unused.length}`,
      `Protected by dynamic prefix: ${protectedRows.length}`,
      '',
      '## Unused candidates',
      '',
      ...(
        unused.length
          ? unused.map(row => `- \`${row.key}\` - en.ts:${row.line}`)
          : ['No unused candidates found.']
      ),
      '',
      '## Protected dynamic keys',
      '',
      ...(
        protectedRows.length
          ? protectedRows.map(row => `- \`${row.key}\` - protected by \`${row.protectedByPrefix}.*\``)
          : ['No keys protected by detected dynamic prefixes.']
      ),
      '',
    ].join('\n'),
  )

  fs.writeFileSync(
    path.join(REPORT_DIR, 'i18n-dynamic-review.md'),
    [
      '# i18n dynamic reference review',
      '',
      'These references require human review before deleting matching keys.',
      '',
      '## Detected prefixes',
      '',
      ...(
        manualReview.length
          ? manualReview.map(item => `- ${item.file}: ${item.kind} \`${item.prefix}.*\` from \`${item.sample}\``)
          : ['No dynamic prefixes detected.']
      ),
      '',
      '## Unknown dynamic calls',
      '',
      ...(
        unknownDynamic.length
          ? unknownDynamic.map(item => `- ${item.file}: ${item.kind} from \`${item.sample}\``)
          : ['No unknown dynamic t() calls detected.']
      ),
      '',
    ].join('\n'),
  )

  console.log(`English keys: ${rows.length}`)
  console.log(`Unused candidates: ${unused.length}`)
  console.log(`Protected dynamic keys: ${protectedRows.length}`)
  console.log('Reports written to reports/')
}

main()
