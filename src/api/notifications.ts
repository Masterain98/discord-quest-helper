import { invoke, isTauri } from '@tauri-apps/api/core'

export type NotificationChannel = string
export type NotificationKind = 'quest_started' | 'quest_completed' | 'queue_completed'
export type NotificationEvents = Record<NotificationKind, boolean>
export interface NotificationDraft { fields: Record<string, string>; reset_fields?: string[] }
export interface NotificationError { code: string; field?: string }
export interface NotificationResult { channel: NotificationChannel; success: boolean; code: string; field?: string; at: string }
export interface NotificationField {
  id: string
  label_key: string
  kind: 'text' | 'password' | 'url' | 'numeric' | 'textarea'
  required: boolean
  placeholder: string | null
  template?: { event: NotificationKind; part: 'title' | 'content'; default_key: string }
}
export interface NotificationChannelView {
  id: NotificationChannel
  name_key: string
  description_key: string | null
  icon: string | null
  fields: NotificationField[]
  help_links: { label_key: string; url: string }[]
  enabled: boolean
  configured: boolean
  values: Record<string, string>
  secret_fields_set: string[]
  template_variables?: { name: string; description_key: string; events: NotificationKind[] }[]
}
export interface NotificationSettings {
  events: NotificationEvents
  channels: NotificationChannelView[]
  results: Partial<Record<NotificationChannel, NotificationResult>>
}
export interface QuestNotification {
  id: string
  kind: NotificationKind
  quest_name?: string
  reward_image?: string | null
  estimated_complete_at?: string | null
  succeeded?: number
  failed?: number
  skipped?: number
}
export const getNotificationSettings = () => invoke<NotificationSettings>('get_notification_settings')
export const saveNotificationEvents = (events: NotificationEvents) => invoke<NotificationSettings>('save_notification_events', { events })
export const setNotificationChannelEnabled = (channel: NotificationChannel, enabled: boolean) => invoke<NotificationSettings>('set_notification_channel_enabled', { channel, enabled })
export const saveNotificationChannel = (channel: NotificationChannel, draft: NotificationDraft) => invoke<NotificationSettings>('save_notification_channel', { channel, draft })
export const clearNotificationChannel = (channel: NotificationChannel) => invoke<NotificationSettings>('clear_notification_channel', { channel })
export const testNotificationChannel = (channel: NotificationChannel, draft: NotificationDraft, locale: string, event?: QuestNotification) => invoke<NotificationResult>('test_notification_channel', { channel, draft, locale, event: event ? { ...event, locale } : null })

let currentLocale: () => string = () => 'en'
export function configureNotificationLocale(getLocale: () => string) { currentLocale = getLocale }

export async function dispatchNotification(event: QuestNotification) {
  if (!isTauri()) return
  await invoke('dispatch_notification', { event: { ...event, locale: currentLocale() } })
}
