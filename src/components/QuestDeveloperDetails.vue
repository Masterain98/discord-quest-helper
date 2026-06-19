<script setup lang="ts">
import { computed, ref } from 'vue'
import type { Quest } from '@/api/tauri'
import { getQuestTasks } from '@/utils/questTasks'
import { Button } from '@/components/ui/button'
import { Check, ChevronDown, Copy } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

const props = defineProps<{
  quest: Quest
}>()

const { t } = useI18n()
const open = ref(false)
const copied = ref<string | null>(null)

const tasks = computed(() => getQuestTasks(props.quest))
const rewards = computed(() => props.quest.config.rewards_config?.rewards || [])
const taskSummary = computed(() => tasks.value.map(task => `${task.key}:${task.type}${task.target ? `:${task.target}` : ''}`).join(', '))
const rewardSkuSummary = computed(() => rewards.value.map(reward => `${reward.sku_id || 'no-sku'}:type-${reward.type}`).join(', '))
const featuresText = computed(() => props.quest.config.features?.join(', ') || 'none')

function metadataState(value?: string | null): string {
  if (value == null) return 'absent'
  return `present, length ${value.length}`
}

const trafficRaw = computed(() => props.quest.traffic_metadata_raw ?? props.quest.config.traffic_metadata_raw)
const trafficSealed = computed(() => props.quest.traffic_metadata_sealed ?? props.quest.config.traffic_metadata_sealed)
const ctaLink = computed(() => props.quest.config.cta_config?.link || null)

async function copyValue(value: string, key: string) {
  await navigator.clipboard.writeText(value)
  copied.value = key
  setTimeout(() => {
    copied.value = null
  }, 1500)
}
</script>

<template>
  <div class="rounded-md border border-border/60 bg-muted/20">
    <button
      type="button"
      class="flex w-full items-center justify-between px-3 py-2 text-left text-xs font-medium text-muted-foreground hover:text-foreground"
      @click="open = !open"
    >
      <span>{{ t('debug.developer_details') }}</span>
      <ChevronDown class="h-3.5 w-3.5 transition-transform" :class="{ 'rotate-180': open }" />
    </button>

    <div v-if="open" class="border-t border-border/60 px-3 py-3">
      <dl class="grid grid-cols-[8rem_minmax(0,1fr)_auto] gap-x-2 gap-y-2 text-xs">
        <dt class="text-muted-foreground">Quest ID</dt>
        <dd class="min-w-0 break-all font-mono">{{ quest.id }}</dd>
        <dd>
          <Button variant="ghost" size="icon" class="h-6 w-6" @click="copyValue(quest.id, 'quest')">
            <Check v-if="copied === 'quest'" class="h-3 w-3 text-green-500" />
            <Copy v-else class="h-3 w-3" />
          </Button>
        </dd>

        <dt class="text-muted-foreground">Config ID</dt>
        <dd class="min-w-0 break-all font-mono">{{ quest.config.id || 'none' }}</dd>
        <dd />

        <dt class="text-muted-foreground">Application</dt>
        <dd class="min-w-0 break-all font-mono">
          {{ quest.config.application?.id || 'none' }}
          <span v-if="quest.config.application?.name" class="text-muted-foreground">({{ quest.config.application.name }})</span>
        </dd>
        <dd />

        <dt class="text-muted-foreground">Tasks</dt>
        <dd class="min-w-0 break-all font-mono">{{ taskSummary || 'none' }}</dd>
        <dd />

        <dt class="text-muted-foreground">Rewards</dt>
        <dd class="min-w-0 break-all font-mono">{{ rewardSkuSummary || 'none' }}</dd>
        <dd />

        <dt class="text-muted-foreground">Features</dt>
        <dd class="min-w-0 break-all font-mono">{{ featuresText }}</dd>
        <dd />

        <dt class="text-muted-foreground">Share Policy</dt>
        <dd class="min-w-0 break-all font-mono">{{ quest.config.share_policy ?? 'none' }}</dd>
        <dd />

        <dt class="text-muted-foreground">CTA Link</dt>
        <dd class="min-w-0 break-all font-mono">{{ ctaLink || 'none' }}</dd>
        <dd>
          <Button v-if="ctaLink" variant="ghost" size="icon" class="h-6 w-6" @click="copyValue(ctaLink, 'cta')">
            <Check v-if="copied === 'cta'" class="h-3 w-3 text-green-500" />
            <Copy v-else class="h-3 w-3" />
          </Button>
        </dd>

        <dt class="text-muted-foreground">Preview</dt>
        <dd class="min-w-0 break-all font-mono">{{ quest.config.preview == null ? 'absent' : 'present' }}</dd>
        <dd />

        <dt class="text-muted-foreground">Targeted Content</dt>
        <dd class="min-w-0 break-all font-mono">{{ quest.config.targeted_content == null ? 'absent' : 'present' }}</dd>
        <dd />

        <dt class="text-muted-foreground">Traffic Raw</dt>
        <dd class="min-w-0 break-all font-mono">{{ metadataState(trafficRaw) }}</dd>
        <dd />

        <dt class="text-muted-foreground">Traffic Sealed</dt>
        <dd class="min-w-0 break-all font-mono">{{ metadataState(trafficSealed) }}</dd>
        <dd />
      </dl>
    </div>
  </div>
</template>
