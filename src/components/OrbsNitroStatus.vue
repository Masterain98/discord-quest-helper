<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useQuestsStore } from '@/stores/quests'
import { useI18n } from 'vue-i18n'
import { Button } from '@/components/ui/button'
import { RotateCw } from 'lucide-vue-next'
import { cn } from '@/lib/utils'

const { t, locale } = useI18n()
const authStore = useAuthStore()
const questsStore = useQuestsStore()

const showOrbs = computed(() => questsStore.showOrbsBalance)
const showNitro = computed(() => authStore.nitroStatus !== null)
const refreshIconSpinning = ref(false)
const stopRefreshIconAfterRotation = ref(false)

watch(
  () => questsStore.orbsBalanceLoading,
  (loading) => {
    if (loading) {
      refreshIconSpinning.value = true
      stopRefreshIconAfterRotation.value = false
      return
    }

    if (refreshIconSpinning.value) {
      stopRefreshIconAfterRotation.value = true
    }
  },
  { immediate: true }
)

function handleRefreshIconIteration() {
  if (stopRefreshIconAfterRotation.value && !questsStore.orbsBalanceLoading) {
    refreshIconSpinning.value = false
    stopRefreshIconAfterRotation.value = false
  }
}

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

const compactClaimText = computed(() => {
  const claim = authStore.nextOrbsClaim
  if (!claim) return ''

  const unit = {
    days: 'day',
    hours: 'hour',
    minutes: 'minute',
  }[claim.unit] as Intl.RelativeTimeFormatUnit

  return new Intl.RelativeTimeFormat(locale.value, {
    numeric: 'always',
    style: 'narrow',
  }).format(claim.value, unit)
})
</script>

<template>
  <div
    v-if="showOrbs || showNitro"
    class="orbs-nitro-status inline-flex min-h-[4.5rem] w-fit max-w-full items-center gap-3 rounded-full border border-violet-500/20 bg-violet-500/10 px-4 py-2.5 text-violet-700 transition-colors hover:bg-violet-500/[0.13] dark:text-violet-300"
  >
    <span class="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-background/65 shadow-sm">
      <img
        :src="showOrbs ? '/icons/orbs.png' : '/icons/nitro.svg'"
        alt=""
        class="h-5 w-5 object-contain"
        aria-hidden="true"
      />
    </span>

    <div class="min-w-0">
      <div v-if="showOrbs" class="flex items-center gap-1.5">
        <span class="sr-only">{{ t('home.current_orbs') }}:</span>
        <span class="whitespace-nowrap text-xl font-semibold leading-none tabular-nums" aria-live="polite">
          {{ questsStore.orbsBalance == null ? '—' : questsStore.orbsBalance.toLocaleString() }}
        </span>
        <Button
          variant="ghost"
          size="icon"
          class="h-7 w-7 shrink-0 rounded-full hover:bg-background/70"
          @click="questsStore.fetchOrbsBalance(true)"
          :disabled="questsStore.orbsBalanceLoading || !authStore.user"
          :aria-label="t('general.refresh')"
        >
          <RotateCw
            :class="cn('h-3.5 w-3.5', refreshIconSpinning && 'animate-spin')"
            @animationiteration="handleRefreshIconIteration"
          />
        </Button>
      </div>

      <div
        v-if="showNitro"
        :class="['mt-1 flex min-w-0 items-center gap-1 text-[10px] leading-tight', !showOrbs && 'mt-0']"
        :title="claimText || t('home.nitro_next_orbs_title')"
      >
        <img
          v-if="showOrbs"
          src="/icons/nitro.svg"
          alt=""
          class="h-3 w-3 shrink-0 object-contain"
          aria-hidden="true"
        />
        <span :class="['shrink-0 font-semibold', authStore.nitroStatus?.class]">
          {{ authStore.nitroStatus?.label }}
        </span>
        <span v-if="compactClaimText" class="whitespace-nowrap text-muted-foreground">
          · {{ compactClaimText }}
        </span>
      </div>
    </div>
  </div>
</template>
