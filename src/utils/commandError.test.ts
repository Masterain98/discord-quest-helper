import { describe, expect, it } from 'vitest'
import { commandErrorMessage, parseCommandError } from './commandError'

describe('parseCommandError', () => {
  it('preserves structured Tauri command errors', () => {
    const parsed = parseCommandError({
      code: 'port_occupied',
      params: { port: 9223 },
      message: 'CDP port 9223 is already occupied.',
    })
    expect(parsed).toEqual({
      code: 'port_occupied',
      params: { port: 9223 },
      message: 'CDP port 9223 is already occupied.',
      rawType: 'object',
    })
  })

  it('handles Error and string values', () => {
    expect(commandErrorMessage(new Error('boom'))).toBe('boom')
    expect(commandErrorMessage('plain error')).toBe('plain error')
  })

  it('never collapses objects to [object Object]', () => {
    expect(commandErrorMessage({ reason: 'unknown' })).toBe('{"reason":"unknown"}')
    expect(commandErrorMessage({})).toBe('Unknown command error')
    expect(commandErrorMessage(null)).not.toBe('[object Object]')
  })
})
