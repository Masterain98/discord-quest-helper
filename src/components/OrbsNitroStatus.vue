<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useQuestsStore } from '@/stores/quests'
import { useI18n } from 'vue-i18n'
import { RotateCw } from 'lucide-vue-next'
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
const compactBalance = computed(() => questsStore.orbsBalance == null ? '—' : new Intl.NumberFormat(locale.value, { notation: 'compact', maximumFractionDigits: 1 }).format(questsStore.orbsBalance))
const label = computed(() => showOrbs.value ? `${t('home.current_orbs')}: ${balance.value}` : authStore.nitroStatus?.label ?? t('home.nitro_next_orbs_title'))
const claimText = computed(() => {
  const claim = authStore.nextOrbsClaim
  return claim ? t(`home.nitro_next_orbs_${claim.unit}`, { [claim.unit]: claim.value }, claim.value) : ''
})

// The account summary belongs to the persistent shell, so starting on Settings
// or switching pages does not depend on Home mounting or repeat cached requests.
watch(() => authStore.user?.id, id => {
  if (!id) return
  void questsStore.fetchOrbsBalance().catch(() => undefined)
  void authStore.fetchNitroProgramReward().catch(() => undefined)
}, { immediate: true })

async function refreshBalance() {
  try { await questsStore.fetchOrbsBalance(true) } catch { /* Display the store's error state below. */ }
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
      <h3 class="min-w-0 flex-1 text-sm font-semibold">{{ showOrbs ? t('home.current_orbs') : authStore.nitroStatus?.label }}</h3>
      <button v-if="showOrbs" type="button" class="rail-panel-action" :aria-label="t('general.refresh')" :disabled="questsStore.orbsBalanceLoading || !authStore.user" @click="refreshBalance">
        <RotateCw class="h-4 w-4" :class="questsStore.orbsBalanceLoading && 'animate-spin'" aria-hidden="true" />
      </button>
    </div>
    <div v-if="showOrbs" class="rail-balance-surface" :aria-busy="questsStore.orbsBalanceLoading">
      <p class="rail-balance-value" aria-live="polite">{{ balance }}</p>
      <span class="text-xs text-muted-foreground">{{ t('home.current_orbs') }}</span>
      <span v-if="questsStore.orbsBalanceLoading" class="sr-only">{{ t('general.loading') }}</span>
    </div>
    <p v-if="showOrbs && questsStore.orbsBalanceError" class="mt-3 text-xs leading-relaxed text-destructive" role="alert">{{ t('sidebar.orbs_unavailable') }}</p>
    <div v-if="showNitro" class="rail-nitro-summary">
      <div class="flex items-center gap-2">
        <img src="/icons/nitro.svg" alt="" class="h-4 w-4 object-contain" />
        <span :class="['text-xs font-semibold', authStore.nitroStatus?.class]">{{ authStore.nitroStatus?.label }}</span>
      </div>
      <p class="mt-2 text-xs leading-relaxed text-muted-foreground">{{ claimText || t('home.nitro_next_orbs_title') }}</p>
    </div>
  </RailPopover>
</template>
