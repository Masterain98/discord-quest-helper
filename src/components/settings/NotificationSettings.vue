<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { Bell, CheckCircle2, ChevronDown, CircleAlert, LoaderCircle, Monitor, Send, Settings2 } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { listen } from '@tauri-apps/api/event'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem } from '@/components/ui/dropdown-menu'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import SettingsSectionCard from './SettingsSectionCard.vue'
import SettingRow from './SettingRow.vue'
import { open } from '@tauri-apps/plugin-shell'
import { useQuestsStore } from '@/stores/quests'
import { notificationRewardImage, estimatedCompletionAt } from '@/utils/questNotifications'
import { channelDraft, clearDraftSecrets, cloneDraft, fieldRequired, inputType, resetDraftField, setDraftField, unknownTemplateVariables } from '@/utils/notificationChannels'
import {
  getNotificationSettings, saveNotificationEvents, saveNotificationChannel,
  setNotificationChannelEnabled, clearNotificationChannel, testNotificationChannel,
  type NotificationChannel, type NotificationKind, type NotificationSettings,
  type NotificationDraft, type NotificationResult, type QuestNotification, type NotificationError, type NotificationField,
} from '@/api/notifications'

const { t, tm, locale } = useI18n()
const props = defineProps<{ debugModeEnabled: boolean }>()
const quests = useQuestsStore()
const testDialogOpen = ref(false)
const testKind = ref<'test' | NotificationKind>('test')
const testQuestId = ref('')
const pendingTest = ref<{ channel: NotificationChannel; draft: NotificationDraft } | null>(null)
const settings = ref<NotificationSettings | null>(null)
const error = ref('')
const busy = ref(false)
const configuredChannelId = ref<NotificationChannel | null>(null)
const configuredChannel = computed(() => settings.value?.channels.find(channel => channel.id === configuredChannelId.value))
const errorField = ref<string>()
const dialogOpen = ref(false)
const draft = ref<NotificationDraft>({ fields: {} })
const dialogError = ref('')
const saveState = ref<'idle' | 'saving' | 'saved'>('idle')
const testing = ref(false)
const testResult = ref<NotificationResult | null>(null)
const channels = computed(() => settings.value?.channels ?? [])
const events: NotificationKind[] = ['quest_started', 'quest_completed', 'queue_completed']
const templateEvent = ref<NotificationKind>('quest_completed')
const variableHint = { variable: '{quest-name}', escaped: '{{quest-name}}' }
const ordinaryFields = computed(() => configuredChannel.value?.fields.filter(field => !field.template) ?? [])
const templateFields = computed(() => configuredChannel.value?.fields.filter(field => field.template?.event === templateEvent.value) ?? [])
const hasTemplates = computed(() => configuredChannel.value?.fields.some(field => field.template) ?? false)
function templated(channel?: NotificationChannel) {
  return channels.value.find(item => item.id === channel)?.fields.some(field => field.template) ?? false
}
function templateValue(field: NotificationField) {
  if (field.id in draft.value.fields) return draft.value.fields[field.id]
  const value = field.template ? tm(field.template.default_key) : ''
  return typeof value === 'string' ? value : ''
}
const unknownVariables = computed(() => [...new Set(templateFields.value.flatMap(field =>
  unknownTemplateVariables(templateValue(field), configuredChannel.value?.template_variables?.map(variable => variable.name) ?? []),
))])
const availableVariables = computed(() => configuredChannel.value?.template_variables?.filter(variable => variable.events.includes(templateEvent.value)) ?? [])
const testOptions = computed(() => [
  ...(!templated(pendingTest.value?.channel) ? [{ value: 'test', label: t('notifications.message_test') }] : []),
  ...events.map(value => ({ value, label: t(`notifications.${value}`) })),
])
const testQuestOptions = computed(() => [
  { value: '', label: t('notifications.sample_quest') },
  ...quests.quests.map(quest => ({ value: quest.id, label: quest.config.messages.quest_name })),
])
const icons: Record<string, typeof Bell> = { monitor: Monitor, send: Send, bell: Bell }
const hasSavedConfig = computed(() => !!configuredChannel.value && (configuredChannel.value.configured || configuredChannel.value.secret_fields_set.length > 0 || Object.values(configuredChannel.value.values).some(Boolean) || configuredChannel.value.fields.some(field => field.template && Object.prototype.hasOwnProperty.call(configuredChannel.value!.values, field.id))))
const dialogBusy = computed(() => testing.value || saveState.value === 'saving')
let disposed = false
let unlisten: (() => void) | undefined
async function openExternal(url: string) {
  try { await open(url) } catch { dialogError.value = t('notifications.errors.unknown') }
}

function message(code: unknown) {
  const value = typeof code === 'string' ? code : (code as NotificationError | null)?.code ?? ''
  return t(`notifications.errors.${['storage', 'secret_store', 'not_configured', 'invalid_channel', 'invalid_config', 'invalid_webhook', 'invalid_thread', 'invalid_token', 'invalid_target', 'system_unavailable', 'network', 'timeout', 'unauthorized', 'forbidden', 'not_found', 'rate_limited', 'rejected', 'invalid_response'].includes(value) ? value : 'unknown'}`)
}
async function load() {
  busy.value = true
  error.value = ''
  try { settings.value = await getNotificationSettings() }
  catch (e) { error.value = message(e) }
  finally { busy.value = false }
}
function resultText(result: NotificationResult) {
  return result.success ? t(result.code === 'submitted' ? (result.channel === 'system' ? 'notifications.submitted' : 'notifications.accepted') : 'notifications.sent') : message(result)
}
function configure(channel: NotificationChannel) {
  const selected = settings.value?.channels.find(item => item.id === channel)
  if (!selected) return
  configuredChannelId.value = channel
  draft.value = channelDraft(selected)
  templateEvent.value = 'quest_completed'
  dialogError.value = ''; errorField.value = undefined
  saveState.value = 'idle'; testResult.value = null
  dialogOpen.value = true
}
function close(open: boolean) {
  if (dialogBusy.value) return
  dialogOpen.value = open
  if (!open) { draft.value = { fields: {} }; errorField.value = undefined }
}
async function toggleEvent(kind: NotificationKind, enabled: boolean) {
  if (!settings.value) return
  busy.value = true; error.value = ''
  try { settings.value = await saveNotificationEvents({ ...settings.value.events, [kind]: enabled }) }
  catch (e) { error.value = message(e) }
  finally { busy.value = false }
}
async function toggleChannel(channel: NotificationChannel, enabled: boolean) {
  if (!settings.value) return
  const selected = settings.value.channels.find(item => item.id === channel)
  if (selected?.fields.length && enabled && !selected.configured) { configure(channel); return }
  busy.value = true; error.value = ''
  try { settings.value = await setNotificationChannelEnabled(channel, enabled) }
  catch (e) { error.value = message(e) }
  finally { busy.value = false }
}
async function saveChannel() {
  if (!configuredChannel.value) return
  saveState.value = 'saving'; dialogError.value = ''; testResult.value = null; errorField.value = undefined
  try {
    settings.value = await saveNotificationChannel(configuredChannel.value.id, cloneDraft(draft.value))
    draft.value = channelDraft(configuredChannel.value)
    saveState.value = 'saved'
  } catch (e) { dialogError.value = message(e); errorField.value = (e as NotificationError)?.field; saveState.value = 'idle' }
}
async function clearChannel() {
  if (!configuredChannel.value) return
  saveState.value = 'saving'; dialogError.value = ''; testResult.value = null; errorField.value = undefined
  try {
    settings.value = await clearNotificationChannel(configuredChannel.value.id)
    draft.value = configuredChannel.value ? channelDraft(configuredChannel.value) : { fields: {} }
    saveState.value = 'saved'
    testResult.value = null
  } catch (e) { dialogError.value = message(e); errorField.value = (e as NotificationError)?.field; saveState.value = 'idle' }
}
function test(channel: NotificationChannel) {
  const value: NotificationDraft = channel === configuredChannelId.value ? cloneDraft(draft.value) : { fields: {} }
  if (props.debugModeEnabled || templated(channel)) {
    pendingTest.value = { channel, draft: value }
    if (templated(channel)) testKind.value = templateEvent.value
    testQuestId.value = quests.activeQuestId ?? quests.quests[0]?.id ?? ''
    testDialogOpen.value = true
  } else {
    void performTest(channel, value)
  }
}
function closeTest(open: boolean) {
  if (testing.value) return
  testDialogOpen.value = open
  if (!open) {
    if (pendingTest.value) clearDraftSecrets(pendingTest.value.draft, channels.value.find(channel => channel.id === pendingTest.value?.channel))
    pendingTest.value = null
  }
}
async function sendSelectedTest() {
  const pending = pendingTest.value
  if (!pending) return
  let event: QuestNotification | undefined
  if ((props.debugModeEnabled || templated(pending.channel)) && testKind.value !== 'test') {
    const quest = quests.quests.find(q => q.id === testQuestId.value)
    event = {
      id: crypto.randomUUID(), kind: testKind.value,
      ...(testKind.value === 'queue_completed' ? { succeeded: 2, failed: 1, skipped: 1 } : {
        quest_name: quest?.config.messages.quest_name || t('notifications.sample_quest'),
        reward_image: quest ? notificationRewardImage(quest) : '/icons/orbs.png',
        estimated_complete_at: testKind.value === 'quest_started' ? estimatedCompletionAt(300) : null,
      }),
    }
  }
  await performTest(pending.channel, pending.draft, event)
  closeTest(false)
}
async function performTest(channel: NotificationChannel, value: NotificationDraft, event?: QuestNotification) {
  testing.value = true; testResult.value = null; dialogError.value = ''; error.value = ''
  try {
    const result = await testNotificationChannel(channel, value, locale.value, event)
    testResult.value = result
    errorField.value = result.field
    if (settings.value) settings.value.results[channel] = result
  } catch (e) {
    if (channel !== configuredChannelId.value) error.value = message(e)
    else { dialogError.value = message(e); errorField.value = (e as NotificationError)?.field }
  } finally {
    const selected = channels.value.find(item => item.id === channel)
    clearDraftSecrets(value, selected)
    if (channel === configuredChannelId.value) clearDraftSecrets(draft.value, selected)
    testing.value = false
  }
}
watch(() => props.debugModeEnabled, enabled => { if (!enabled && !templated(pendingTest.value?.channel)) closeTest(false) })
function changed() {
  saveState.value = 'idle'; testResult.value = null; dialogError.value = ''; errorField.value = undefined
}
function input(field: string, event: Event) {
  setDraftField(draft.value, field, (event.target as HTMLInputElement | HTMLTextAreaElement).value)
  changed()
}
function resetField(field: string) {
  resetDraftField(draft.value, field)
  changed()
}
onMounted(async () => {
  await load()
  try {
    const stop = await listen<NotificationResult>('notification-delivery', ({ payload }) => {
      if (settings.value) settings.value.results[payload.channel] = payload
    })
    if (disposed) stop(); else unlisten = stop
  } catch { /* Browser previews do not have the native event bridge. */ }
})
onUnmounted(() => { disposed = true; unlisten?.(); draft.value = { fields: {} }; if (pendingTest.value) pendingTest.value.draft.fields = {}; pendingTest.value = null })
</script>

<template>
  <SettingsSectionCard :title="t('settings.nav_notifications')" :description="t('notifications.description')" :icon="Bell">
    <p v-if="error" role="alert" class="text-sm text-destructive">{{ error }}</p>
    <Button v-if="!settings" variant="outline" :disabled="busy" @click="load">
      <LoaderCircle v-if="busy" class="mr-2 h-4 w-4 animate-spin" />{{ t('notifications.reload') }}
    </Button>
    <template v-else>
      <div>
        <h3 class="mb-2 text-sm font-semibold">{{ t('notifications.content') }}</h3>
        <SettingRow v-for="event in events" :key="event" :label="t(`notifications.${event}`)" :description="t(`notifications.${event}_desc`)">
          <Switch :model-value="settings.events[event]" :disabled="busy" :aria-label="t(`notifications.${event}`)" @update:model-value="toggleEvent(event, $event)" />
        </SettingRow>
      </div>
      <div>
        <h3 class="text-sm font-semibold">{{ t('notifications.channels') }}</h3>
        <p class="mt-1 mb-2 text-xs leading-relaxed text-muted-foreground">{{ t('notifications.channels_desc') }}</p>
        <div v-for="channel in channels" :key="channel.id" :id="`notification-channel-${channel.id}`" class="border-t border-border/60 py-4 first:border-t-0" data-settings-search :data-search-label="t(channel.name_key)">
          <div class="flex flex-wrap items-center justify-between gap-3">
            <div class="flex min-w-0 items-center gap-3">
              <component :is="icons[channel.icon ?? ''] ?? Bell" class="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden="true" />
              <div>
                <p class="text-sm font-medium">{{ t(channel.name_key) }}</p>
                <p class="mt-1 text-xs text-muted-foreground">{{ channel.description_key ? t(channel.description_key) : t(channel.configured ? 'notifications.configured' : 'notifications.not_configured') }}</p>
              </div>
            </div>
            <div class="flex items-center gap-3">
              <Button v-if="channel.fields.length > 0" variant="outline" size="sm" :disabled="busy || testing" :aria-label="t('notifications.configure_channel', { channel: t(channel.name_key) })" @click="configure(channel.id)">
                <Settings2 class="mr-1.5 h-3.5 w-3.5" />{{ t('notifications.configure') }}
              </Button>
              <Button v-else variant="outline" size="sm" :disabled="testing || busy" @click="test(channel.id)">
                <LoaderCircle v-if="testing && !dialogOpen" class="mr-1.5 h-3.5 w-3.5 animate-spin" />{{ t('notifications.test') }}
              </Button>
              <Switch :model-value="channel.enabled" :disabled="busy" :aria-label="t('notifications.enable_channel', { channel: t(channel.name_key) })" @update:model-value="toggleChannel(channel.id, $event)" />
            </div>
          </div>
          <p v-if="settings.results[channel.id]" role="status" class="mt-2 flex items-start gap-1.5 text-xs leading-relaxed" :class="settings.results[channel.id]!.success ? 'text-emerald-700 dark:text-emerald-300' : 'text-destructive'">
            <component :is="settings.results[channel.id]!.success ? CheckCircle2 : CircleAlert" class="mt-0.5 h-3.5 w-3.5 shrink-0" />
            <span>{{ resultText(settings.results[channel.id]!) }} · {{ new Date(settings.results[channel.id]!.at).toLocaleTimeString(locale) }}</span>
          </p>
        </div>
        <p class="text-xs leading-relaxed text-muted-foreground">{{ t('notifications.system_hint') }}</p>
      </div>
    </template>
  </SettingsSectionCard>

  <Dialog :open="dialogOpen" @update:open="close">
    <DialogContent class="max-h-[calc(100dvh-2rem)] overflow-y-auto" @escape-key-down="dialogBusy && $event.preventDefault()" @pointer-down-outside="dialogBusy && $event.preventDefault()">
      <DialogHeader>
        <DialogTitle>{{ t('notifications.configure_channel', { channel: configuredChannel ? t(configuredChannel.name_key) : '' }) }}</DialogTitle>
        <DialogDescription>{{ t('notifications.dialog_desc') }}</DialogDescription>
      </DialogHeader>
      <form v-if="configuredChannel" class="grid gap-4" autocomplete="off" @submit.prevent="saveChannel">
        <div v-for="field in ordinaryFields" :key="field.id" class="grid gap-2">
          <Label :for="'notification-field-' + field.id">{{ t(field.label_key) }}</Label>
          <textarea v-if="field.kind === 'textarea'" :id="'notification-field-' + field.id" :value="draft.fields[field.id] ?? ''"
            rows="5" class="w-full min-w-0 resize-y rounded-md border border-input bg-background px-3 py-2 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
            :disabled="dialogBusy" :required="fieldRequired(field, configuredChannel)" :aria-invalid="errorField === field.id || undefined"
            :aria-describedby="errorField === field.id ? 'notification-config-error' : undefined" @input="input(field.id, $event)" />
          <Input v-else :id="'notification-field-' + field.id" :type="inputType(field)" :inputmode="field.kind === 'numeric' ? 'numeric' : undefined"
            :autocomplete="field.kind === 'password' ? 'new-password' : 'off'" spellcheck="false"
            :model-value="draft.fields[field.id] ?? ''" :required="fieldRequired(field, configuredChannel)"
            :placeholder="field.kind === 'password' && configuredChannel.secret_fields_set.includes(field.id) && !draft.reset_fields?.includes(field.id) ? t('notifications.secret_saved') : field.placeholder ?? (!field.required ? t('notifications.optional') : undefined)"
            :aria-invalid="errorField === field.id || undefined" :aria-describedby="errorField === field.id ? 'notification-config-error' : undefined"
            :disabled="dialogBusy" @input="input(field.id, $event)" />
          <button v-if="field.kind === 'password' && !field.required && !draft.reset_fields?.includes(field.id) && (configuredChannel.secret_fields_set.includes(field.id) || draft.fields[field.id])"
            type="button" class="justify-self-start text-xs text-destructive underline underline-offset-4" :disabled="dialogBusy" @click="resetField(field.id)">{{ t('notifications.remove_secret') }}</button>
          <p v-if="field.kind === 'password' && draft.reset_fields?.includes(field.id)" role="status" class="text-xs text-muted-foreground">{{ t('notifications.secret_remove_pending') }}</p>
        </div>
        <fieldset v-if="hasTemplates" class="grid min-w-0 gap-4 border-t border-border pt-4">
          <legend class="text-sm font-semibold">{{ t('notifications.templates') }}</legend>
          <p class="text-xs leading-relaxed text-muted-foreground">{{ t('notifications.templates_desc') }}</p>
          <div class="grid gap-2">
            <Label for="notification-template-event">{{ t('notifications.template_event') }}</Label>
            <select id="notification-template-event" v-model="templateEvent" :disabled="dialogBusy" class="h-10 w-full min-w-0 rounded-md border border-input bg-background px-3 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
              <option v-for="event in events" :key="event" :value="event">{{ t('notifications.' + event) }}</option>
            </select>
          </div>
          <div v-for="field in templateFields" :key="field.id" class="grid min-w-0 gap-2">
            <div class="flex flex-wrap items-center justify-between gap-2">
              <Label :for="'notification-field-' + field.id">{{ t(field.label_key) }}</Label>
              <button type="button" class="text-xs text-primary underline underline-offset-4" :disabled="dialogBusy" @click="resetField(field.id)">{{ t('notifications.restore_default') }}</button>
            </div>
            <textarea v-if="field.kind === 'textarea'" :id="'notification-field-' + field.id" :value="templateValue(field)"
              rows="5" class="w-full min-w-0 resize-y rounded-md border border-input bg-background px-3 py-2 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
              :disabled="dialogBusy" :aria-invalid="errorField === field.id || undefined" :aria-describedby="errorField === field.id ? 'notification-config-error' : undefined" spellcheck="false" @input="input(field.id, $event)" />
            <Input v-else :id="'notification-field-' + field.id" :model-value="templateValue(field)" :disabled="dialogBusy" :aria-invalid="errorField === field.id || undefined" :aria-describedby="errorField === field.id ? 'notification-config-error' : undefined" spellcheck="false" @input="input(field.id, $event)" />
            <p class="text-xs text-muted-foreground">{{ t(field.id in draft.fields ? 'notifications.template_custom' : 'notifications.template_default') }}</p>
          </div>
          <p v-if="unknownVariables.length" role="status" class="break-words text-xs text-amber-700 dark:text-amber-300">{{ t('notifications.unknown_variables', { variables: unknownVariables.map(name => '{' + name + '}').join(', ') }) }}</p>
          <details class="min-w-0 text-xs">
            <summary class="cursor-pointer font-medium">{{ t('notifications.template_variables') }}</summary>
            <p class="mt-2 leading-relaxed text-muted-foreground">{{ t('notifications.variables_hint', variableHint) }}</p>
            <dl class="mt-3 grid gap-2">
              <div v-for="variable in availableVariables" :key="variable.name" class="grid gap-0.5">
                <dt class="break-all font-mono">{{ '{' + variable.name + '}' }}</dt>
                <dd class="text-muted-foreground">{{ t(variable.description_key) }}</dd>
              </div>
            </dl>
          </details>
        </fieldset>
        <div class="flex flex-wrap gap-x-4 gap-y-2 text-xs">
          <a v-for="link in configuredChannel.help_links" :key="link.url" :href="link.url" class="text-primary underline underline-offset-4" @click.prevent="openExternal(link.url)">{{ t(link.label_key) }}</a>
          <button v-if="hasSavedConfig" type="button" class="text-destructive underline underline-offset-4" :disabled="dialogBusy" @click="clearChannel">{{ t('notifications.clear_config') }}</button>
        </div>
        <p v-if="dialogError" id="notification-config-error" role="alert" class="text-sm text-destructive">{{ dialogError }}</p>
        <p v-if="saveState === 'saved'" role="status" class="text-sm text-emerald-700 dark:text-emerald-300">{{ t('notifications.saved') }}</p>
        <p v-if="testResult" :id="testResult.success ? undefined : 'notification-config-error'" role="status" class="text-sm" :class="testResult.success ? 'text-emerald-700 dark:text-emerald-300' : 'text-destructive'">{{ resultText(testResult) }}</p>
        <DialogFooter class="flex-wrap gap-2">
          <Button type="button" variant="ghost" :disabled="dialogBusy" @click="close(false)">{{ t('notifications.cancel') }}</Button>
          <Button type="button" variant="outline" :disabled="dialogBusy" @click="test(configuredChannel.id)"><LoaderCircle v-if="testing" class="mr-2 h-4 w-4 animate-spin" />{{ t('notifications.test') }}</Button>
          <Button type="submit" :disabled="dialogBusy"><LoaderCircle v-if="saveState === 'saving'" class="mr-2 h-4 w-4 animate-spin" />{{ t('notifications.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
  <Dialog :open="testDialogOpen" @update:open="closeTest">
    <DialogContent @escape-key-down="testing && $event.preventDefault()" @pointer-down-outside="testing && $event.preventDefault()">
      <DialogHeader>
        <DialogTitle>{{ t('notifications.test_type_title') }}</DialogTitle>
        <DialogDescription>{{ t('notifications.test_type_desc') }}</DialogDescription>
      </DialogHeader>
      <div class="grid gap-2">
        <Label id="notification-test-kind-label" for="notification-test-kind">{{ t('notifications.test_type') }}</Label>
        <DropdownMenu>
          <DropdownMenuTrigger as-child :disabled="testing">
            <Button id="notification-test-kind" variant="outline" class="w-full justify-between font-normal" aria-labelledby="notification-test-kind-label">
              <span class="truncate">{{ testOptions.find(option => option.value === testKind)?.label }}</span>
              <ChevronDown class="ml-2 h-4 w-4 shrink-0 text-muted-foreground" aria-hidden="true" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" class="max-h-[min(20rem,var(--radix-dropdown-menu-content-available-height))] w-[var(--radix-dropdown-menu-trigger-width)] overflow-y-auto">
            <DropdownMenuRadioGroup v-model="testKind">
              <DropdownMenuRadioItem v-for="option in testOptions" :key="option.value" :value="option.value">{{ option.label }}</DropdownMenuRadioItem>
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <div v-if="testKind === 'quest_started' || testKind === 'quest_completed'" class="grid gap-2">
        <Label id="notification-test-quest-label" for="notification-test-quest">{{ t('notifications.test_quest') }}</Label>
        <DropdownMenu>
          <DropdownMenuTrigger as-child :disabled="testing">
            <Button id="notification-test-quest" variant="outline" class="w-full justify-between font-normal" aria-labelledby="notification-test-quest-label">
              <span class="truncate">{{ testQuestOptions.find(option => option.value === testQuestId)?.label }}</span>
              <ChevronDown class="ml-2 h-4 w-4 shrink-0 text-muted-foreground" aria-hidden="true" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" class="max-h-[min(20rem,var(--radix-dropdown-menu-content-available-height))] w-[var(--radix-dropdown-menu-trigger-width)] overflow-y-auto">
            <DropdownMenuRadioGroup v-model="testQuestId">
              <DropdownMenuRadioItem v-for="option in testQuestOptions" :key="option.value" :value="option.value">{{ option.label }}</DropdownMenuRadioItem>
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <DialogFooter class="gap-2">
        <Button variant="ghost" :disabled="testing" @click="closeTest(false)">{{ t('notifications.cancel') }}</Button>
        <Button :disabled="testing" @click="sendSelectedTest"><LoaderCircle v-if="testing" class="mr-2 h-4 w-4 animate-spin" />{{ t('notifications.test') }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
