import { describe, expect, it } from 'vitest'
import fixture from '../../tests/fixtures/notification-channels.json'
import type { NotificationSettings } from '@/api/notifications'
import { channelDraft, clearDraftSecrets, fieldRequired, inputType } from './notificationChannels'

describe('descriptor driven channel forms', () => {
  // Rust asserts this shared JSON against the actual serialized settings contract.
  const settings = fixture as unknown as NotificationSettings
  const channel = settings.channels.find(item => item.id === 'fixture')!
  it('supports a fourth channel and all field kinds without channel-specific code', () => {
    const draft = channelDraft(channel)
    expect(Object.keys(draft.fields)).toEqual(['endpoint', 'api_key', 'signing_key', 'recipient'])
    expect(draft.fields.endpoint).toBe('https://example.invalid/push')
    expect(draft.fields.recipient).toBe('18446744073709551615')
    expect(draft.fields.api_key).toBe('')
    expect(draft.fields.signing_key).toBe('')
    expect(channel.fields.map(inputType)).toEqual(['url', 'password', 'password', 'text'])
  })
  it('requires new credentials but keeps both saved password fields optional', () => {
    const fresh = { ...channel, secret_fields_set: [] }
    expect(channel.fields.map(field => fieldRequired(field, fresh))).toEqual([true, true, true, false])
    expect(channel.fields.map(field => fieldRequired(field, channel))).toEqual([true, false, false, false])
  })
  it('clears every password after a test while retaining ordinary form values', () => {
    const draft = channelDraft(channel)
    draft.fields.api_key = 'fixture-api-value'; draft.fields.signing_key = 'fixture-signing-value'
    clearDraftSecrets(draft, channel)
    expect(draft.fields.api_key).toBe(''); expect(draft.fields.signing_key).toBe('')
    expect(draft.fields.recipient).toBe('18446744073709551615')
    expect(channel.secret_fields_set).toEqual(['api_key', 'signing_key'])
  })
  it('only exposes descriptor fields and never initializes a password from values', () => {
    const value = channelDraft({ ...channel, values: { ...channel.values, api_key: 'not-for-ui', unregistered: 'not-for-ui' } })
    expect(value.fields.api_key).toBe(''); expect(value.fields).not.toHaveProperty('unregistered')
    clearDraftSecrets(value)
    expect(value.fields).toEqual({})
  })
})
