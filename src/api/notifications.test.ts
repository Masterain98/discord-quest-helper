import { beforeEach, describe, expect, it, vi } from 'vitest'
import { testNotificationChannel, saveNotificationChannel, clearNotificationChannel, setNotificationChannelEnabled, type NotificationKind, type QuestNotification } from './notifications'
import requests from '../../tests/fixtures/notification-test-requests.json'

const invoke = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ invoke, isTauri: () => true }))
beforeEach(() => invoke.mockReset())

describe('notification test IPC contract', () => {
  it('passes an arbitrary registered channel and string-valued fields through IPC', async () => {
    const draft = { fields: { endpoint: 'https://example.invalid/push', recipient: '18446744073709551615', api_key: '' } }
    await saveNotificationChannel('fixture', draft)
    expect(invoke).toHaveBeenLastCalledWith('save_notification_channel', { channel: 'fixture', draft })
    await setNotificationChannelEnabled('fixture', true)
    expect(invoke).toHaveBeenLastCalledWith('set_notification_channel_enabled', { channel: 'fixture', enabled: true })
    await clearNotificationChannel('fixture')
    expect(invoke).toHaveBeenLastCalledWith('clear_notification_channel', { channel: 'fixture' })
  })
  it('preserves sparse templates, explicit empty text and reset instructions through IPC', async () => {
    const draft = { fields: { quest_completed_title: '', quest_completed_content: ' \n{quest-name}\n ' }, reset_fields: ['bearer_token', 'quest_started_content'] }
    await saveNotificationChannel('custom_webhook', draft)
    expect(invoke).toHaveBeenLastCalledWith('save_notification_channel', { channel: 'custom_webhook', draft })
    await testNotificationChannel('custom_webhook', draft, 'zh')
    expect(invoke).toHaveBeenLastCalledWith('test_notification_channel', { channel: 'custom_webhook', draft, locale: 'zh', event: null })
  })
  it.each(requests)('sends the native request schema for $event.kind', async request => {
    const event: QuestNotification | undefined = request.event
      ? { ...request.event, kind: request.event.kind as NotificationKind }
      : undefined
    // The UI event has no locale; the API must attach it inside the native event.
    if (event) delete (event as QuestNotification & { locale?: string }).locale
    await testNotificationChannel('system', request.draft, request.locale, event)
    expect(invoke).toHaveBeenCalledWith('test_notification_channel', request)
    if (event) expect(event).not.toHaveProperty('locale')
  })
})
