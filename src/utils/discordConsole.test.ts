import { describe, expect, it } from 'vitest'
import { formatConsoleException, formatRemoteObject, remoteObjectIds } from './discordConsole'

describe('console values', () => {
  it('preserves special JavaScript types', () => {
    expect(formatRemoteObject({ type: 'undefined' })).toBe('undefined')
    expect(formatRemoteObject({ type: 'object', subtype: 'null', value: null })).toBe('null')
    expect(formatRemoteObject({ type: 'number', unserializableValue: 'NaN' })).toBe('NaN')
    expect(formatRemoteObject({ type: 'bigint', unserializableValue: '123n' })).toBe('123n')
    expect(formatRemoteObject({ type: 'boolean', value: false })).toBe('false')
    expect(formatRemoteObject({ type: 'string', value: '' })).toBe('')
  })
  it('formats previews and exception stack traces', () => {
    expect(formatRemoteObject({ type: 'object', description: 'Object', preview: { properties: [{ name: 'x', type: 'number', value: '1' }], overflow: true } })).toBe('Object { x: 1, … }')
    expect(formatConsoleException({ text: 'Uncaught', exception: { type: 'object', description: 'Error: test' },
      stackTrace: { callFrames: [{ functionName: 'f', url: 'test.js', lineNumber: 0, columnNumber: 2 }] } })).toBe('Error: test\n    at f (test.js:1:3)')
  })
  it('collects descriptor handles while excluding arbitrary by-value data', () => {
    expect(remoteObjectIds({ result: [{ name: 'child', value: { type: 'object', objectId: 'child' } }],
      other: { type: 'object', value: { objectId: 'user-data' } } })).toEqual(['child'])
  })
})
