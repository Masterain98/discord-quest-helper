<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { usePreferredReducedMotion } from '@vueuse/core'
import { useAuthStore } from '@/stores/auth'
import { useQuestsStore } from '@/stores/quests'
import { useI18n } from 'vue-i18n'
import { AlertCircle, Clock3, RotateCw, Trophy } from 'lucide-vue-next'
import RailPopover from './RailPopover.vue'

const { t, locale } = useI18n()
defineProps<{ suspended?: boolean }>()
const authStore = useAuthStore()
const questsStore = useQuestsStore()
const open = ref(false)
const showOrbs = computed(() => questsStore.showOrbsBalance)
const showNitro = computed(() => authStore.nitroStatus !== null)
watch(() => showOrbs.value || showNitro.value, visible => { if (!visible) open.value = false })
const balance = computed(() => questsStore.orbsBalance == null ? '—' : questsStore.orbsBalance.toLocaleString(locale.value))
const totalRedeemed = computed(() => questsStore.orbsTotalRedeemed == null ? '—' : questsStore.orbsTotalRedeemed.toLocaleString(locale.value))
const compactBalance = computed(() => questsStore.orbsBalance == null ? '—' : new Intl.NumberFormat(locale.value, { notation: 'compact', maximumFractionDigits: 1 }).format(questsStore.orbsBalance))
const label = computed(() => showOrbs.value ? `${t('home.current_orbs')}: ${balance.value}` : authStore.nitroStatus?.label ?? t('home.nitro_next_orbs_title'))
const claimText = computed(() => {
  const claim = authStore.nextOrbsClaim
  return claim ? t(`home.nitro_next_orbs_${claim.unit}`, { [claim.unit]: claim.value }, claim.value) : ''
})
const refreshing = computed(() => questsStore.orbsSummaryLoading || authStore.programRewardLoading)
const reducedMotion = usePreferredReducedMotion()
const refreshSpinning = ref(false)
// Keep the animation alive after the request settles. Its iteration event
// stops it at a complete turn, where the static icon has the same orientation.
watch([refreshing, open, reducedMotion], ([loading, visible, motion]) => {
  if (!visible || motion === 'reduce') refreshSpinning.value = false
  else if (loading) refreshSpinning.value = true
}, { flush: 'sync' })

function finishRefreshTurn(event: AnimationEvent) {
  if (event.animationName === 'orbs-refresh-turn' && !refreshing.value) {
    refreshSpinning.value = false
  }
}
const countdownText = computed(() => {
  if (authStore.programRewardLoading && !claimText.value) return t('general.loading')
  if (authStore.programRewardError) return t('home.orbs_airdrop_unavailable')
  return claimText.value || t('home.orbs_no_airdrop')
})

// The account summary belongs to the persistent shell, so starting on Settings
// or switching pages does not depend on Home mounting or repeat cached requests.
watch(() => authStore.user?.id, id => {
  if (!id) return
  void questsStore.fetchOrbsSummary().catch(() => undefined)
  void authStore.fetchNitroProgramReward().catch(() => undefined)
}, { immediate: true })

async function refreshSummary() {
  if (!authStore.user || refreshing.value || refreshSpinning.value) return
  await Promise.allSettled([
    ...(showOrbs.value ? [questsStore.fetchOrbsSummary(true)] : []),
    ...(showNitro.value ? [authStore.fetchNitroProgramReward(true)] : []),
  ])
}
</script>

<template>
  <RailPopover v-if="showOrbs || showNitro" v-model:open="open" :suspended="suspended" :label="label">
    <template #trigger>
      <button type="button" :aria-label="label" :class="['app-rail-button rail-tool-button rail-orbs-trigger', open && 'rail-tool-button-open']">
        <img :src="showOrbs ? '/icons/orbs.png' : '/icons/nitro.svg'" alt="" class="h-4 w-4 object-contain" aria-hidden="true" />
        <span class="rail-tool-caption tabular-nums" aria-hidden="true">{{ showOrbs ? (questsStore.orbsBalanceLoading && questsStore.orbsBalance == null ? '…' : compactBalance) : authStore.nitroStatus?.label }}</span>
      </button>
    </template>
    <div class="rail-panel-heading">
      <span class="rail-panel-glyph"><img :src="showOrbs ? '/icons/orbs.png' : '/icons/nitro.svg'" alt="" class="h-5 w-5 object-contain" /></span>
      <h3 class="min-w-0 flex-1 text-sm font-semibold">{{ showOrbs ? t('home.orbs_title') : authStore.nitroStatus?.label }}</h3>
      <button type="button" class="rail-panel-action orbs-refresh" :aria-label="t('general.refresh')" :title="t('general.refresh')" :disabled="refreshing || refreshSpinning || !authStore.user" @click="refreshSummary">
        <RotateCw class="h-4 w-4" :class="refreshSpinning && 'orbs-refreshing'" :stroke-width="1.7" aria-hidden="true" @animationiteration="finishRefreshTurn" />
      </button>
    </div>
    <dl v-if="showOrbs" class="orbs-overview ui-recess">
      <div class="orbs-balance" :aria-busy="questsStore.orbsBalanceLoading">
        <dt class="orbs-metric-label">{{ t('home.current_orbs') }}</dt>
        <dd class="orbs-balance-value" aria-live="polite">
          <span v-if="questsStore.orbsBalanceLoading && questsStore.orbsBalance == null" class="orbs-skeleton orbs-balance-skeleton"><span class="sr-only">{{ t('general.loading') }}</span></span>
          <template v-else>{{ balance }}</template>
        </dd>
        <dd v-if="questsStore.orbsBalanceError" class="orbs-metric-error" role="alert"><AlertCircle aria-hidden="true" />{{ t('sidebar.orbs_unavailable') }}</dd>
      </div>
      <div class="orbs-redeemed" :aria-busy="questsStore.orbsTotalRedeemedLoading">
        <dt class="orbs-redeemed-label"><Trophy class="orbs-redeemed-icon" :stroke-width="1.7" aria-hidden="true" /><span>{{ t('home.orbs_total_redeemed') }}</span></dt>
        <dd class="orbs-redeemed-value" aria-live="polite">
          <span v-if="questsStore.orbsTotalRedeemedLoading && questsStore.orbsTotalRedeemed == null" class="orbs-skeleton orbs-redeemed-skeleton"><span class="sr-only">{{ t('general.loading') }}</span></span>
          <template v-else>{{ totalRedeemed }}</template>
        </dd>
        <dd v-if="questsStore.orbsTotalRedeemedError" class="orbs-metric-error" role="alert"><AlertCircle aria-hidden="true" />{{ t('home.orbs_redeemed_unavailable') }}</dd>
      </div>
    </dl>
    <section v-if="showNitro" class="orbs-nitro" :aria-label="authStore.nitroStatus?.label">
      <div class="orbs-nitro-heading">
        <span class="orbs-nitro-glyph ui-icon-tile"><img src="/icons/nitro.svg" alt="" class="h-4 w-4 object-contain" /></span>
        <span class="text-xs font-semibold">{{ authStore.nitroStatus?.label }}</span>
      </div>
      <p class="orbs-countdown" :class="authStore.programRewardError && 'orbs-countdown-error'" :role="authStore.programRewardError ? 'alert' : 'status'">
        <Clock3 class="h-3.5 w-3.5 shrink-0" :stroke-width="1.7" aria-hidden="true" />
        <span>{{ countdownText }}</span>
      </p>
    </section>
  </RailPopover>
</template>
