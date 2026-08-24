import { describe, expect, it } from 'vitest'
import { sanitizeRuntimeIdentityAuditExport } from './runtimeIdentityAudit'

describe('runtime identity audit export', () => {
  it('redacts home paths and drops sensitive fields recursively', () => {
    const sanitized = sanitizeRuntimeIdentityAuditExport({
      path: '/Users/alice/Library/Application Support/blueorbit/runtime',
      nested: {
        authorization: 'secret',
        cookie: 'secret',
        userId: '123',
        raw: 'local-only',
        safe: '/home/bob/.local/share/blueorbit',
      },
    })
    expect(sanitized).toEqual({
      path: '$HOME/Library/Application Support/blueorbit/runtime',
      nested: { safe: '$HOME/.local/share/blueorbit' },
    })
  })

  it('redacts Windows profiles case-insensitively with either separator', () => {
    expect(sanitizeRuntimeIdentityAuditExport({
      backslash: String.raw`c:\users\Alice\AppData\Local\blueorbit`,
      slash: 'D:/USERS/Bob/AppData/Local/blueorbit',
    })).toEqual({
      backslash: String.raw`$HOME\AppData\Local\blueorbit`,
      slash: '$HOME/AppData/Local/blueorbit',
    })
  })
})
