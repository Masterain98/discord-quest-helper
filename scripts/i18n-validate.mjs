import fs from 'node:fs'
import path from 'node:path'
import ts from 'typescript'

const ROOT = process.cwd()
const LOCALE_DIR = path.join(ROOT, 'src/locales')
const REPORT_DIR = path.join(ROOT, 'reports')
const EN_FILE = path.join(LOCALE_DIR, 'en.ts')
const PLACEHOLDER_RE = /\{[a-zA-Z0-9_]+\}/g
const TODO_RE = /\b(?:TODO|FIXME|TRANSLATE|UNTRANSLATED)\b/
const TOKEN_RE = /\[\[/

function propNameToString(name) {
  if (ts.isIdentifier(name)) return name.text
  if (ts.isStringLiteral(name) || ts.isNumericLiteral(name)) return name.text
  return null
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

function flattenLocaleObject(sourceFile, objectLiteral, prefix = [], out = new Map(), errors = []) {
  for (const prop of objectLiteral.properties) {
    if (!ts.isPropertyAssignment(prop)) continue

    const name = propNameToString(prop.name)
    if (!name) continue

    const next = [...prefix, name]
    const key = next.join('.')
    const init = prop.initializer
    const line = ts.getLineAndCharacterOfPosition(sourceFile, prop.getStart(sourceFile)).line + 1

    if (ts.isObjectLiteralExpression(init)) {
      flattenLocaleObject(sourceFile, init, next, out, errors)
    } else if (ts.isStringLiteral(init) || ts.isNoSubstitutionTemplateLiteral(init)) {
      out.set(key, {
        key,
        value: init.text,
        line,
      })
    } else {
      errors.push({
        key,
        line,
        issue: 'leaf value is not a string literal',
      })
    }
  }

  return { values: out, errors }
}

function readLocale(file) {
  const full = path.join(LOCALE_DIR, file)
  const text = fs.readFileSync(full, 'utf8')
  const sourceFile = ts.createSourceFile(full, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS)
  const obj = findDefaultExportObject(sourceFile)

  if (!obj) {
    throw new Error(`Cannot find default export object in ${file}`)
  }

  const { values, errors } = flattenLocaleObject(sourceFile, obj)
  return {
    locale: file.replace(/\.ts$/, ''),
    file,
    values,
    errors,
  }
}

function placeholders(value) {
  return new Set(value.match(PLACEHOLDER_RE) ?? [])
}

function sameSet(a, b) {
  if (a.size !== b.size) return false
  for (const item of a) {
    if (!b.has(item)) return false
  }
  return true
}

function sortedDiff(a, b) {
  return [...a].filter(item => !b.has(item)).sort()
}

function main() {
  fs.mkdirSync(REPORT_DIR, { recursive: true })

  const localeFiles = fs.readdirSync(LOCALE_DIR)
    .filter(file => file.endsWith('.ts'))
    .sort((a, b) => {
      if (a === 'en.ts') return -1
      if (b === 'en.ts') return 1
      return a.localeCompare(b)
    })

  const baseline = readLocale(path.basename(EN_FILE))
  const baselineKeys = new Set(baseline.values.keys())
  const locales = localeFiles.filter(file => file !== 'en.ts').map(readLocale)

  const missing = []
  const extra = []
  const placeholderMismatch = []
  const emptyStrings = []
  const markerErrors = []
  const nonStringErrors = []
  const warnings = []

  for (const error of baseline.errors) {
    nonStringErrors.push({ locale: 'en', ...error })
  }

  for (const item of baseline.values.values()) {
    if (!item.value.trim()) emptyStrings.push({ locale: 'en', key: item.key, line: item.line })
    if (TODO_RE.test(item.value) || TOKEN_RE.test(item.value)) {
      markerErrors.push({ locale: 'en', key: item.key, line: item.line, value: item.value })
    }
  }

  for (const locale of locales) {
    const localeKeys = new Set(locale.values.keys())

    for (const key of sortedDiff(baselineKeys, localeKeys)) {
      missing.push({ locale: locale.locale, key })
    }

    for (const key of sortedDiff(localeKeys, baselineKeys)) {
      extra.push({ locale: locale.locale, key })
    }

    for (const error of locale.errors) {
      nonStringErrors.push({ locale: locale.locale, ...error })
    }

    for (const [key, item] of locale.values) {
      if (!item.value.trim()) emptyStrings.push({ locale: locale.locale, key, line: item.line })
      if (TODO_RE.test(item.value) || TOKEN_RE.test(item.value)) {
        markerErrors.push({ locale: locale.locale, key, line: item.line, value: item.value })
      }

      const enItem = baseline.values.get(key)
      if (!enItem) continue

      const enPlaceholders = placeholders(enItem.value)
      const targetPlaceholders = placeholders(item.value)
      if (!sameSet(enPlaceholders, targetPlaceholders)) {
        placeholderMismatch.push({
          locale: locale.locale,
          key,
          expected: [...enPlaceholders].sort(),
          actual: [...targetPlaceholders].sort(),
        })
      }

      if (
        locale.locale !== 'en'
        && item.value === enItem.value
        && item.value.length > 18
        && !/^(Discord|CDP|API|RPC|Token|X-Super-Properties|x-super-properties|Tauri|Vue 3|TailwindCSS|vue-i18n|shadcn-vue)/.test(item.value)
      ) {
        warnings.push({
          locale: locale.locale,
          key,
          issue: 'value matches English baseline',
          value: item.value,
        })
      }
    }
  }

  const errorCount = missing.length + extra.length + placeholderMismatch.length + emptyStrings.length + markerErrors.length + nonStringErrors.length
  const report = [
    '# i18n validation report',
    '',
    'Baseline: src/locales/en.ts',
    '',
    'Locales checked:',
    ...locales.map(locale => `- ${locale.locale}`),
    '',
    'Result:',
    `- missing keys: ${missing.length}`,
    `- extra keys: ${extra.length}`,
    `- placeholder mismatch: ${placeholderMismatch.length}`,
    `- empty strings: ${emptyStrings.length}`,
    `- marker errors: ${markerErrors.length}`,
    `- non-string leaf values: ${nonStringErrors.length}`,
    `- warnings: ${warnings.length}`,
    '',
    '## Missing keys',
    '',
    ...(missing.length ? missing.map(item => `- ${item.locale}: \`${item.key}\``) : ['None.']),
    '',
    '## Extra keys',
    '',
    ...(extra.length ? extra.map(item => `- ${item.locale}: \`${item.key}\``) : ['None.']),
    '',
    '## Placeholder mismatches',
    '',
    ...(placeholderMismatch.length
      ? placeholderMismatch.map(item => `- ${item.locale}: \`${item.key}\` expected ${item.expected.join(', ') || '(none)'} actual ${item.actual.join(', ') || '(none)'}`)
      : ['None.']),
    '',
    '## Empty strings',
    '',
    ...(emptyStrings.length ? emptyStrings.map(item => `- ${item.locale}: \`${item.key}\` line ${item.line}`) : ['None.']),
    '',
    '## Marker errors',
    '',
    ...(markerErrors.length ? markerErrors.map(item => `- ${item.locale}: \`${item.key}\` line ${item.line}`) : ['None.']),
    '',
    '## Non-string leaf values',
    '',
    ...(nonStringErrors.length ? nonStringErrors.map(item => `- ${item.locale}: \`${item.key}\` line ${item.line} - ${item.issue}`) : ['None.']),
    '',
    '## Soft warnings',
    '',
    ...(warnings.length ? warnings.map(item => `- ${item.locale}: \`${item.key}\` - ${item.issue}`) : ['None.']),
    '',
  ].join('\n')

  fs.writeFileSync(path.join(REPORT_DIR, 'i18n-validate.md'), report)
  console.log(`Locales checked: ${locales.map(locale => locale.locale).join(', ')}`)
  console.log(`Errors: ${errorCount}`)
  console.log(`Warnings: ${warnings.length}`)

  if (errorCount > 0) {
    process.exitCode = 1
  }
}

main()
