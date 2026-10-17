<script setup lang="ts">
import { computed, ref } from 'vue'
import { ChevronRight, Loader2 } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import type { ConsoleProperties, RemoteObject } from '@/api/discordConsole'
import { useDiscordConsoleStore, type ConsoleEntry } from '@/stores/discordConsole'
import { formatRemoteObject } from '@/utils/discordConsole'

defineOptions({ name: 'ConsoleObject' })
const props = defineProps<{ object: RemoteObject; entry: ConsoleEntry; ancestors?: string[] }>()
const store = useDiscordConsoleStore()
const { t } = useI18n()
const expanded = ref(false)
const loading = ref(false)
const failed = ref(false)
const result = ref<ConsoleProperties | null>(null)
const expired = computed(() => props.entry.sessionId !== store.session?.sessionId)
const circular = computed(() => !!props.object.objectId && props.ancestors?.includes(props.object.objectId))
const children = computed(() => [...(result.value?.result ?? []), ...(result.value?.internalProperties ?? [])])

async function toggle() {
  if (expired.value || loading.value || circular.value || !props.object.objectId) return
  expanded.value = !expanded.value
  if (!expanded.value || result.value) return
  loading.value = true
  try {
    result.value = await store.expand(props.entry, props.object.objectId)
    failed.value = !!result.value.exceptionDetails
  } catch { failed.value = true }
  finally { loading.value = false }
}
</script>

<template>
  <span class="inline-block max-w-full align-top">
    <button v-if="object.objectId" type="button" class="inline-flex max-w-full items-start gap-1 text-left hover:text-primary disabled:cursor-default"
      :disabled="expired || circular" :aria-expanded="expanded" @click="toggle">
      <Loader2 v-if="loading" class="mt-0.5 h-3.5 w-3.5 shrink-0 animate-spin" />
      <ChevronRight v-else class="mt-0.5 h-3.5 w-3.5 shrink-0 transition-transform" :class="expanded && 'rotate-90'" />
      <span class="whitespace-pre-wrap break-all">{{ formatRemoteObject(object) }}</span>
    </button>
    <span v-else class="whitespace-pre-wrap break-all" :class="object.type === 'string' ? 'text-green-700 dark:text-green-400' : 'text-foreground'">{{ formatRemoteObject(object) }}</span>
    <span v-if="object.objectId && expired" class="ml-2 text-xs text-muted-foreground">{{ t('discord_console.expired') }}</span>
    <span v-else-if="circular" class="ml-2 text-muted-foreground">{{ t('discord_console.circular') }}</span>
    <div v-if="expanded && !expired" class="ml-2 border-l border-border pl-3">
      <p v-if="result" class="mb-1 text-[11px] text-muted-foreground">{{ t('discord_console.property_snapshot') }}</p>
      <p v-if="failed" class="text-destructive">{{ t('discord_console.property_error') }}</p>
      <p v-else-if="result && !children.length" class="text-muted-foreground">{{ t('discord_console.no_properties') }}</p>
      <div v-for="(property, index) in children" :key="`${property.name}:${index}`" class="py-0.5">
        <span class="text-muted-foreground">{{ property.name }}: </span>
        <ConsoleObject v-if="property.value" :object="property.value" :entry="entry" :ancestors="[...(ancestors ?? []), object.objectId!]" />
        <span v-else>{{ [property.get?.type === 'function' && 'getter', property.set?.type === 'function' && 'setter'].filter(Boolean).join(' / ') }}</span>
      </div>
    </div>
  </span>
</template>
