import fs from 'node:fs'
import path from 'node:path'

const ROOT = process.cwd()
const LOCALE_DIR = path.join(ROOT, 'src', 'locales')
const REPORT_DIR = path.join(ROOT, 'reports')
const SOURCE_LOCALE = 'en'
const PLACEHOLDER_RE = /\{[a-zA-Z0-9_]+\}/g
const TODO_RE = /\b(?:TODO|FIXME|TRANSLATE|UNTRANSLATED)\b/
const TOKEN_RE = /\[\[/

function isPlainObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function readJson(fileName) {
  const filePath = path.join(LOCALE_DIR, fileName)
  try {
    return JSON.parse(fs.readFileSync(filePath, 'utf8'))
  } catch (error) {
    throw new Error(`Invalid JSON in ${fileName}: ${error.message}`)
  }
}

function flattenMessages(value, prefix = '') {
  if (!isPlainObject(value)) {
    throw new Error(`Expected object at ${prefix || '<root>'}`)
  }

  const result = new Map()

  for (const [key, child] of Object.entries(value)) {
    const nextPath = prefix ? `${prefix}.${key}` : key

    if (typeof child === 'string') {
      result.set(nextPath, child)
      continue
    }

    if (isPlainObject(child)) {
      for (const [nestedKey, nestedValue] of flattenMessages(child, nextPath)) {
        result.set(nestedKey, nestedValue)
      }
      continue
    }

    throw new Error(
      `Invalid value at ${nextPath}: expected string or object, got ${Array.isArray(child) ? 'array' : typeof child}`,
    )
  }

  return result
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

  const jsonFiles = fs.readdirSync(LOCALE_DIR)
    .filter(file => file.endsWith('.json'))
    .sort((a, b) => {
      if (a === `${SOURCE_LOCALE}.json`) return -1
      if (b === `${SOURCE_LOCALE}.json`) return 1
      return a.localeCompare(b)
    })

  const tsLocaleFiles = fs.readdirSync(LOCALE_DIR)
    .filter(file => file.endsWith('.ts') && file !== 'meta.ts')

  if (tsLocaleFiles.length > 0) {
    console.error(`WARNING: Unexpected TypeScript locale files remain: ${tsLocaleFiles.join(', ')}`)
  }

  if (!jsonFiles.includes(`${SOURCE_LOCALE}.json`)) {
    throw new Error(`Missing source locale: ${SOURCE_LOCALE}.json`)
  }

  const baseline = flattenMessages(readJson(`${SOURCE_LOCALE}.json`))
  const baselineKeys = new Set(baseline.keys())

  const locales = jsonFiles
    .filter(file => file !== `${SOURCE_LOCALE}.json`)
    .map(file => ({
      locale: file.replace(/\.json$/, ''),
      file,
      values: flattenMessages(readJson(file)),
    }))

  const missing = []
  const extra = []
  const placeholderMismatch = []
  const emptyStrings = []
  const markerErrors = []
  const nonStringErrors = []
  const warnings = []

  for (const [key, value] of baseline) {
    if (!value.trim()) emptyStrings.push({ locale: SOURCE_LOCALE, key })
    if (TODO_RE.test(value) || TOKEN_RE.test(value)) {
      markerErrors.push({ locale: SOURCE_LOCALE, key, value })
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

    for (const [key, value] of locale.values) {
      if (!value.trim()) emptyStrings.push({ locale: locale.locale, key })
      if (TODO_RE.test(value) || TOKEN_RE.test(value)) {
        markerErrors.push({ locale: locale.locale, key, value })
      }

      const enValue = baseline.get(key)
      if (!enValue) continue

      const enPlaceholders = placeholders(enValue)
      const targetPlaceholders = placeholders(value)
      if (!sameSet(enPlaceholders, targetPlaceholders)) {
        placeholderMismatch.push({
          locale: locale.locale,
          key,
          expected: [...enPlaceholders].sort(),
          actual: [...targetPlaceholders].sort(),
        })
      }

      if (
        locale.locale !== SOURCE_LOCALE
        && value === enValue
        && value.length > 18
        && !/^(Discord|CDP|API|RPC|Token|X-Super-Properties|x-super-properties|Tauri|Vue 3|TailwindCSS|vue-i18n|shadcn-vue)/.test(value)
      ) {
        warnings.push({
          locale: locale.locale,
          key,
          issue: 'value matches English baseline',
          value,
        })
      }
    }
  }

  const errorCount = missing.length + extra.length + placeholderMismatch.length + emptyStrings.length + markerErrors.length + nonStringErrors.length
  const report = [
    '# i18n validation report',
    '',
    `Baseline: src/locales/${SOURCE_LOCALE}.json`,
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
    ...(emptyStrings.length ? emptyStrings.map(item => `- ${item.locale}: \`${item.key}\``) : ['None.']),
    '',
    '## Marker errors',
    '',
    ...(markerErrors.length ? markerErrors.map(item => `- ${item.locale}: \`${item.key}\``) : ['None.']),
    '',
    '## Non-string leaf values',
    '',
    ...(nonStringErrors.length ? nonStringErrors.map(item => `- ${item.locale}: \`${item.key}\` - ${item.issue}`) : ['None.']),
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
