import type { NotificationChannelView, NotificationDraft, NotificationField } from '@/api/notifications'

/** Every input stays a string, including IDs longer than JavaScript's safe integers. */
export function channelDraft(channel: NotificationChannelView): NotificationDraft {
  return { fields: Object.fromEntries(channel.fields
    .filter(field => !field.template || Object.prototype.hasOwnProperty.call(channel.values, field.id))
    .map(field => [field.id, field.kind === 'password' ? '' : channel.values[field.id] ?? ''])) }
}

export function cloneDraft(draft: NotificationDraft): NotificationDraft {
  return { fields: { ...draft.fields }, ...(draft.reset_fields?.length ? { reset_fields: [...draft.reset_fields] } : {}) }
}

export function resetDraftField(draft: NotificationDraft, id: string) {
  delete draft.fields[id]
  draft.reset_fields = [...new Set([...(draft.reset_fields ?? []), id])]
}

export function setDraftField(draft: NotificationDraft, id: string, value: string) {
  draft.fields[id] = value
  draft.reset_fields = draft.reset_fields?.filter(field => field !== id)
}

export function unknownTemplateVariables(text: string, names: string[]): string[] {
  const known = new Set(names)
  const unknown = new Set<string>()
  for (const match of text.matchAll(/\{\{([A-Za-z][A-Za-z0-9_-]*)\}\}|\{([A-Za-z][A-Za-z0-9_-]*)\}/g)) {
    if (match[2] && !known.has(match[2])) unknown.add(match[2])
  }
  return [...unknown]
}

export function clearDraftSecrets(draft: NotificationDraft, channel?: NotificationChannelView) {
  if (!channel) { draft.fields = {}; delete draft.reset_fields; return }
  for (const field of channel.fields) if (field.kind === 'password') {
    if (draft.reset_fields?.includes(field.id)) delete draft.fields[field.id]
    else draft.fields[field.id] = ''
  }
}

export function fieldRequired(field: NotificationField, channel: NotificationChannelView) {
  return field.required && !(field.kind === 'password' && channel.secret_fields_set.includes(field.id))
}

export function inputType(field: NotificationField) {
  return field.kind === 'numeric' ? 'text' : field.kind
}
