import { describe, expect, it } from 'vitest'
import fixture from '../../tests/fixtures/notification-channels.json'
import templateCases from '../../tests/fixtures/notification-template-cases.json'
import type { NotificationSettings } from '@/api/notifications'
import { channelDraft, clearDraftSecrets, cloneDraft, fieldRequired, inputType, resetDraftField, setDraftField, unknownTemplateVariables } from './notificationChannels'

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

describe('Webhook template drafts', () => {
  const channel = (fixture as unknown as NotificationSettings).channels.find(item => item.id === 'custom_webhook')!
  it('keeps defaults absent and preserves explicit empty strings and whitespace', () => {
    const fresh = channelDraft(channel)
    expect(fresh.fields).toEqual({ endpoint: '', bearer_token: '' })
    const saved = channelDraft({ ...channel, values: { quest_completed_title: '', quest_completed_content: ' \n{quest-name}\n ' } })
    expect(saved.fields.quest_completed_title).toBe('')
    expect(saved.fields.quest_completed_content).toBe(' \n{quest-name}\n ')
    expect(saved.fields).not.toHaveProperty('quest_started_title')
  })
  it('resets independently, carries resets into tests and cancels resets on edits', () => {
    const draft = channelDraft(channel)
    setDraftField(draft, 'quest_completed_content', '')
    resetDraftField(draft, 'quest_completed_content')
    resetDraftField(draft, 'bearer_token')
    resetDraftField(draft, 'bearer_token')
    expect(draft.fields).not.toHaveProperty('quest_completed_content')
    expect(draft.reset_fields).toEqual(['quest_completed_content', 'bearer_token'])
    const copied = cloneDraft(draft)
    setDraftField(copied, 'quest_completed_content', '  exact\n')
    expect(copied.reset_fields).toEqual(['bearer_token'])
    expect(draft.reset_fields).toEqual(['quest_completed_content', 'bearer_token'])
    clearDraftSecrets(draft, channel)
    expect(draft.fields).not.toHaveProperty('bearer_token')
    expect(draft.reset_fields).toEqual(['quest_completed_content', 'bearer_token'])
    expect(draft.fields.endpoint).toBe('')
  })
  it.each(templateCases)('warns without blocking or confusing escaped variables: $text', value => {
    expect(unknownTemplateVariables(value.text, ['quest-name', 'timestamp', 'succeeded', 'failed', 'skipped'])).toEqual(value.unknown)
  })
})
