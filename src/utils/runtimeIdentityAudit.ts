const SENSITIVE_KEY = /^(?:authorization|token|cookie|user_?id|raw)$/i

function redactHomePath(value: string): string {
  return value
    .replace(/\/Users\/[^/\\]+(?=[/\\]|$)/g, '$HOME')
    .replace(/\/home\/[^/\\]+(?=[/\\]|$)/g, '$HOME')
    .replace(/[A-Za-z]:[/\\]Users[/\\][^/\\]+(?=[/\\]|$)/gi, '$HOME')
}

export function sanitizeRuntimeIdentityAuditExport(value: unknown): unknown {
  if (typeof value === 'string') return redactHomePath(value)
  if (Array.isArray(value)) return value.map(sanitizeRuntimeIdentityAuditExport)
  if (!value || typeof value !== 'object') return value

  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>)
      .filter(([key]) => !SENSITIVE_KEY.test(key))
      .map(([key, child]) => [key, sanitizeRuntimeIdentityAuditExport(child)]),
  )
}
