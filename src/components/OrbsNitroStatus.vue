<script setup lang="ts">
import { computed } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useQuestsStore } from '@/stores/quests'
import { useI18n } from 'vue-i18n'
import { Button } from '@/components/ui/button'
import { RotateCw } from 'lucide-vue-next'
import { cn } from '@/lib/utils'

const { t } = useI18n()
const authStore = useAuthStore()
const questsStore = useQuestsStore()

const showOrbs = computed(() => questsStore.showOrbsBalance)
const showNitro = computed(() => authStore.nitroStatus !== null)

// Build the countdown label from the structured claim value, switching the
// i18n key based on the chosen unit and locale-aware plural category.
const claimText = computed(() => {
  const claim = authStore.nextOrbsClaim
  if (!claim) return ''

  return t(
    `home.nitro_next_orbs_${claim.unit}`,
    { [claim.unit]: claim.value },
    claim.value
  )
})
</script>

<template>
  <div
    v-if="showOrbs || showNitro"
    class="ml-auto flex shrink-0 items-center gap-1.5 rounded-md border bg-card px-2.5 py-1.5 text-xs"
  >
    <!-- Current Orbs -->
    <template v-if="showOrbs">
      <img src="/icons/orbs.png" alt="" class="h-4 w-4 object-contain" />
      <span class="text-muted-foreground">{{ t('home.current_orbs') }}:</span>
      <span class="font-semibold">
        {{ questsStore.orbsBalance == null ? '—' : questsStore.orbsBalance.toLocaleString() }}
      </span>
      <Button
        variant="ghost"
        size="icon"
        class="h-5 w-5"
        @click="questsStore.fetchOrbsBalance(true)"
        :disabled="questsStore.orbsBalanceLoading || !authStore.user"
        :aria-label="t('general.refresh')"
      >
        <RotateCw :class="cn('h-3 w-3', questsStore.orbsBalanceLoading && 'animate-spin')" />
      </Button>
    </template>

    <!-- Divider -->
    <span v-if="showOrbs && showNitro" class="mx-0.5 h-4 w-px bg-border" />

    <!-- Nitro membership status + monthly Orbs countdown -->
    <span
      v-if="showNitro"
      class="inline-flex items-center gap-1.5"
      :title="t('home.nitro_next_orbs_title')"
    >
      <svg
        viewBox="0 0 24 24"
        class="h-4 w-4 text-violet-500 dark:text-violet-400"
        aria-hidden="true"
      >
        <path
          fill="currentColor"
          fill-rule="evenodd"
          clip-rule="evenodd"
          d="M15 5.5a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM7.5 12a7.5 7.5 0 1 1 15 0 7.5 7.5 0 0 1-15 0Z"
        />
        <path
          fill="currentColor"
          fill-rule="evenodd"
          clip-rule="evenodd"
          d="M6 4.5h9v1H6v-1zM5.5 8h3v1h-3V8zM2 8h1.5v1H2V8zm2 4h4v1H4v-1zm8.584-3.277A.5.5 0 0 1 13 8.5h4a.5.5 0 0 1 .416.223l2 3a.5.5 0 0 1 0 .554l-2 3A.5.5 0 0 1 17 15.5h-4a.5.5 0 0 1-.416-.223l-2-3a.5.5 0 0 1 0-.554l2-3zm.684.777L11.6 12l1.667 2.5h3.464L18.4 12l-1.667-2.5h-3.464z"
        />
      </svg>
      <span :class="['font-semibold', authStore.nitroStatus?.class]">
        {{ authStore.nitroStatus?.label }}
      </span>
      <span
        v-if="claimText"
        class="inline-flex items-center rounded-full border border-violet-400/40 bg-violet-500/10 px-1.5 py-0.5 text-[10px] font-medium text-violet-600 dark:text-violet-300"
      >
        {{ claimText }}
      </span>
    </span>
  </div>
</template>
