export interface ParsedCommandError {
  code: string | null
  message: string
  params: Record<string, unknown> | null
  rawType: string
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function safeFallback(value: unknown): string {
  if (value === null) return 'Unknown error (null)'
  if (value === undefined) return 'Unknown error'
  if (typeof value === 'symbol') return value.description || 'Unknown symbol error'
  if (typeof value === 'object') {
    try {
      const serialized = JSON.stringify(value)
      if (serialized && serialized !== '{}') return serialized
    } catch {
      // Fall through to a stable message instead of exposing [object Object].
    }
    return 'Unknown command error'
  }
  return String(value)
}

export function parseCommandError(value: unknown): ParsedCommandError {
  const rawType = value === null ? 'null' : Array.isArray(value) ? 'array' : typeof value
  if (isRecord(value)) {
    const message = typeof value.message === 'string' && value.message.trim()
      ? value.message
      : value instanceof Error && value.message
        ? value.message
        : safeFallback(value)
    return {
      code: typeof value.code === 'string' && value.code ? value.code : null,
      message,
      params: isRecord(value.params) ? value.params : null,
      rawType: value instanceof Error ? 'Error' : rawType,
    }
  }
  return {
    code: null,
    message: typeof value === 'string' && value ? value : safeFallback(value),
    params: null,
    rawType,
  }
}

export function commandErrorMessage(value: unknown): string {
  return parseCommandError(value).message
}
