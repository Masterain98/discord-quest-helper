<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { Check, ChevronDown, Copy, Loader2, Pause, Play, Power, Terminal, Trash2, Wifi } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuItem } from '@/components/ui/dropdown-menu'
import ConsoleObject from '@/components/console/ConsoleObject.vue'
import { useDiscordConsoleStore, type ConsoleEntry } from '@/stores/discordConsole'
import { useToastStore } from '@/stores/toast'

const store = useDiscordConsoleStore()
const toast = useToastStore()
const { t } = useI18n()
const output = ref<HTMLElement | null>(null)
const editor = ref<HTMLTextAreaElement | null>(null)
const copied = ref<number | null>(null)
const follow = ref(true)
const renderLimit = ref(120)
const renderStart = ref(0)
const loadingOlder = ref(false)
const props = defineProps<{ active: boolean; enabled: boolean; port: number }>()
const emit = defineEmits<{ enable: []; disable: [] }>()
const ready = computed(() => store.connected && !store.executing && !!store.input.trim())
const renderedEntries = computed(() => {
  const total = store.visibleEntries.length
  const start = follow.value ? Math.max(0, total - renderLimit.value) : Math.min(renderStart.value, total)
  return store.visibleEntries.slice(start, start + renderLimit.value)
})
const levels = ['all', 'log', 'info', 'debug', 'warn', 'error'] as const
const errorText = (code: string) => t(`discord_console.errors.${code}`)

watch(() => [props.enabled, props.port] as const, ([enabled, port]) => {
  if (enabled) void store.connect(port)
}, { immediate: true })
watch(() => props.active, async active => {
  if (!active) return
  await nextTick()
  editor.value?.focus()
  if (output.value) output.value.scrollTop = output.value.scrollHeight
}, { immediate: true })
watch(() => [store.search, store.level] as const, async () => {
  renderLimit.value = 120
  renderStart.value = 0
  follow.value = true
  await nextTick()
  if (output.value) output.value.scrollTop = output.value.scrollHeight
})
watch(() => store.visibleEntries[store.visibleEntries.length - 1]?.id, async () => {
  if (!follow.value) return
  await nextTick()
  if (output.value) output.value.scrollTop = output.value.scrollHeight
})
async function trackScroll() {
  const element = output.value
  if (!element) return
  const atBottom = element.scrollHeight - element.scrollTop - element.clientHeight < 40
  if (atBottom) {
    follow.value = true
    return
  }
  if (follow.value) {
    const total = store.visibleEntries.length
    renderStart.value = Math.max(0, total - renderLimit.value)
  }
  follow.value = false
  if (element.scrollTop > 32 || loadingOlder.value || renderStart.value === 0) return

  loadingOlder.value = true
  const previousHeight = element.scrollHeight
  const previousEnd = renderStart.value + renderLimit.value
  renderStart.value = Math.max(0, renderStart.value - 100)
  renderLimit.value = previousEnd - renderStart.value
  await nextTick()
  element.scrollTop += element.scrollHeight - previousHeight
  loadingOlder.value = false
}
function keyboard(event: KeyboardEvent) {
  if (event.isComposing) return
  if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) { event.preventDefault(); void store.execute() }
  if ((event.key === 'ArrowUp' || event.key === 'ArrowDown') && event.altKey) {
    event.preventDefault(); store.recall(event.key === 'ArrowUp' ? -1 : 1)
  }
}
async function copy(entry: ConsoleEntry) {
  try {
    await navigator.clipboard.writeText(entry.errorCode ? errorText(entry.errorCode) : entry.text)
    copied.value = entry.id
  } catch { toast.error({ title: t('discord_console.copy_failed') }) }
}
</script>

<template>
<section class="flex min-h-0 min-w-0 flex-1 flex-col gap-4 pb-4">
  <div class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h2 class="ui-page-heading flex items-center gap-2 text-2xl font-semibold"><Terminal class="h-5 w-5 text-primary" :stroke-width="1.7" />{{ t('nav.discord_console') }}</h2>
        <p class="mt-1 text-sm text-muted-foreground">{{ t('discord_console.description') }}</p>
      </div>
      <div class="flex min-w-0 items-center gap-2 text-sm text-muted-foreground" role="status">
        <Loader2 v-if="store.connecting" class="h-4 w-4 animate-spin" />
        <Wifi v-else class="h-4 w-4" :class="store.connected && 'text-green-600 dark:text-green-400'" />
        <span>{{ t(store.connecting ? 'discord_console.connecting' : store.connected ? 'discord_console.connected' : 'discord_console.disconnected') }}</span>
        <span v-if="store.session" class="max-w-64 truncate" :title="store.session.targetUrl">{{ store.session.targetTitle }} · {{ store.session.port }}</span>
      </div>
    </div>

  <div v-if="!props.enabled" class="ui-panel flex min-h-[clamp(20rem,55vh,38rem)] flex-1 flex-col items-center justify-center rounded-2xl border bg-card px-8 py-12 text-center" role="status">
      <div class="mb-5 flex h-16 w-16 items-center justify-center rounded-2xl border border-muted-foreground/15 bg-muted/50 text-muted-foreground">
        <Terminal class="h-8 w-8" :stroke-width="1.6" aria-hidden="true" />
      </div>
      <h3 class="text-xl font-semibold">{{ t('discord_console.disabled_title') }}</h3>
      <p class="mt-2 max-w-lg text-sm leading-6 text-muted-foreground">{{ t('discord_console.disabled_description') }}</p>
      <Button class="mt-6 gap-2" @click="emit('enable')"><Power class="h-4 w-4" aria-hidden="true" />{{ t('discord_console.enable') }}</Button>
    </div>

    <div v-else class="ui-panel flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-2xl border bg-card">
      <div class="flex flex-wrap items-center gap-2 border-b p-3">
        <Input v-model="store.search" class="h-9 min-w-40 flex-1" :placeholder="t('discord_console.search')" :aria-label="t('discord_console.search')" />
        <DropdownMenu>
          <DropdownMenuTrigger as-child>
            <Button variant="outline" size="sm" class="min-w-28 justify-between gap-2" :aria-label="`${t('discord_console.level')}: ${t(`discord_console.levels.${store.level}`)}`">
              {{ t(`discord_console.levels.${store.level}`) }}<ChevronDown class="h-4 w-4 text-muted-foreground" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" class="min-w-36">
            <DropdownMenuItem v-for="item in levels" :key="item" class="gap-2" @select="store.level = item">
              <Check class="h-4 w-4" :class="store.level !== item && 'invisible'" aria-hidden="true" />
              {{ t(`discord_console.levels.${item}`) }}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <Button :variant="store.paused ? 'secondary' : 'outline'" size="sm" class="gap-2" :aria-pressed="store.paused" @click="store.togglePaused">
          <Play v-if="store.paused" class="h-4 w-4" /><Pause v-else class="h-4 w-4" />
          {{ t(store.paused ? 'discord_console.resume' : 'discord_console.pause') }}
        </Button>
        <Button variant="outline" size="sm" class="gap-2" @click="store.clearOutput"><Trash2 class="h-4 w-4" />{{ t('discord_console.clear') }}</Button>
        <Button variant="outline" size="sm" class="gap-2 text-destructive hover:text-destructive" @click="emit('disable')"><Power class="h-4 w-4" />{{ t('discord_console.disable') }}</Button>
      </div>
      <p v-if="store.paused" class="border-b bg-muted/40 px-3 py-2 text-xs text-muted-foreground" role="status">
        {{ t('discord_console.paused_notice', { count: store.pendingCount }) }}
      </p>

      <div ref="output" class="min-h-0 min-w-0 flex-1 overflow-auto font-mono text-sm" role="region" :aria-label="t('discord_console.output')" tabindex="0" @scroll="trackScroll">
        <p v-if="!store.visibleEntries.length" class="p-6 text-center font-sans text-muted-foreground">{{ t(store.entries.length ? 'discord_console.no_matches' : 'discord_console.empty') }}</p>
        <div v-for="entry in renderedEntries" :key="entry.id" class="group flex items-start gap-2 border-b border-border/40 px-3 py-2"
          :class="entry.level === 'error' ? 'bg-destructive/5 text-destructive' : entry.level === 'warn' ? 'bg-amber-500/5 text-amber-700 dark:text-amber-400' : ''">
          <time class="mt-0.5 shrink-0 text-[10px] text-muted-foreground" :datetime="new Date(entry.timestamp).toISOString()">{{ new Date(entry.timestamp).toLocaleTimeString() }}</time>
          <span class="shrink-0 text-muted-foreground" aria-hidden="true">{{ entry.kind === 'command' ? '›' : entry.kind === 'result' ? '‹' : '·' }}</span>
          <div class="min-w-0 flex-1 whitespace-pre-wrap break-all">
            <span v-if="entry.errorCode">{{ errorText(entry.errorCode) }}</span>
            <template v-else-if="entry.kind !== 'command' && entry.kind !== 'exception' && entry.args.length">
              <ConsoleObject v-for="(object, index) in entry.args" :key="index" :object="object" :entry="entry" class="mr-2" />
            </template>
            <span v-else>{{ entry.text }}</span>
          </div>
          <Button variant="ghost" size="icon" class="h-6 w-6 shrink-0" :title="t('discord_console.copy')" :aria-label="t('discord_console.copy')" @click="copy(entry)">
            <Check v-if="copied === entry.id" class="h-3.5 w-3.5" /><Copy v-else class="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      <div class="border-t p-3">
        <label for="discord-console-expression" class="sr-only">{{ t('discord_console.command') }}</label>
        <textarea id="discord-console-expression" ref="editor" v-model="store.input" spellcheck="false" autocapitalize="off" autocomplete="off"
          class="ui-input min-h-28 w-full resize-y rounded-xl border bg-background p-3 font-mono text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          :placeholder="t('discord_console.placeholder')" @keydown="keyboard" />
        <div class="mt-2 flex flex-wrap items-center justify-between gap-2">
          <p class="text-xs text-muted-foreground">{{ t('discord_console.shortcuts') }}</p>
          <Button class="ui-button-neomorphic gap-2" :disabled="!ready" @click="store.execute">
            <Loader2 v-if="store.executing" class="h-4 w-4 animate-spin" /><Play v-else class="h-4 w-4" />
            {{ t(store.executing ? 'discord_console.executing' : 'discord_console.execute') }}
          </Button>
        </div>
        <p v-if="store.lastError" class="mt-2 text-sm text-destructive" role="alert">{{ errorText(store.lastError) }}</p>
      </div>
    </div>
  </section>
</template>
