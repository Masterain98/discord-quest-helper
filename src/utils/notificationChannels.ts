import type { NotificationChannelView, NotificationDraft, NotificationField } from '@/api/notifications'

/** Every input stays a string, including IDs longer than JavaScript's safe integers. */
export function channelDraft(channel: NotificationChannelView): NotificationDraft {
  return { fields: Object.fromEntries(channel.fields.map(field => [field.id, field.kind === 'password' ? '' : channel.values[field.id] ?? ''])) }
}

export function clearDraftSecrets(draft: NotificationDraft, channel?: NotificationChannelView) {
  if (!channel) { draft.fields = {}; return }
  for (const field of channel.fields) if (field.kind === 'password') draft.fields[field.id] = ''
}

export function fieldRequired(field: NotificationField, channel: NotificationChannelView) {
  return field.required && !(field.kind === 'password' && channel.secret_fields_set.includes(field.id))
}

export function inputType(field: NotificationField) {
  return field.kind === 'numeric' ? 'text' : field.kind
}
