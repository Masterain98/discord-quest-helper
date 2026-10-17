<script setup lang="ts">
import { computed, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import { ChevronDown, Clock, Gift, Link2 } from 'lucide-vue-next'
import QuestTaskBadges from '@/components/QuestTaskBadges.vue'
import { useQuestsStore } from '@/stores/quests'
import { useAuthStore } from '@/stores/auth'
import { groupProgress, groupStats, hasLinkedQuestProgress, questGroupKey, type QuestDisplayGroup } from '@/utils/questGroups'
import { formatQuestReward, type QuestRewardView } from '@/utils/questRewards'

const props = defineProps<{ group: QuestDisplayGroup, expanded: boolean, compact?: boolean }>()
defineEmits<{ 'update:expanded': [value: boolean] }>()
const { t } = useI18n()
const store = useQuestsStore()
const authStore = useAuthStore()
const contentId = useId()
const first = computed(() => props.group.quests[0])
const heroImage = computed(() => {
  const asset = first.value.config.assets?.hero
  if (!asset) return null
  return asset.startsWith('/') || /^https?:\/\//i.test(asset)
    ? asset
    : `https://cdn.discordapp.com/${asset}`
})
const rawRewards = computed(() => first.value.config.rewards_config?.rewards || [])
const rewards = computed(() => rawRewards.value
  .map(reward => formatQuestReward(reward, null, authStore.user?.premium_type)))
function rewardKey(reward: QuestRewardView): string {
  return `${reward.skuId}-${reward.type}-${reward.name}`
}
// Progress describes the entire account group, even when a tab hides members.
// Action slots still receive only the filtered, visible members.
const allMembers = computed(() => {
  const key = questGroupKey(first.value)
  const members = new Map(props.group.quests.map(quest => [quest.id, quest]))
  if (key) for (const quest of store.quests) {
    if (questGroupKey(quest) === key) members.set(quest.id, quest)
  }
  return [...members.values()]
})
const stats = computed(() => groupStats(allMembers.value))
const expiryDate = computed(() => {
  const dates = allMembers.value
    .map(quest => quest.config.expires_at)
    .filter((date): date is string => !!date && Number.isFinite(Date.parse(date)))
  return dates.reduce<string | null>((earliest, date) => {
    if (!earliest || Date.parse(date) < Date.parse(earliest)) return date
    return earliest
  }, null)
})
const expirySummary = computed(() => {
  if (!expiryDate.value) return t('quest.no_expiry')
  const expires = new Date(expiryDate.value)
  const diff = expires.getTime() - Date.now()
  if (diff < 0) return t('quest.expired')
  const days = Math.floor(diff / (1000 * 60 * 60 * 24))
  const hours = Math.floor((diff % (1000 * 60 * 60 * 24)) / (1000 * 60 * 60))
  const date = expires.toLocaleDateString()
  if (days > 0) return t('quest.time_left_days', { date, days, hours })
  return t('quest.time_left_hours', { date, hours })
})
const progress = computed(() => groupProgress(allMembers.value, {
  questId: store.activeQuestId, percentage: store.activeQuestProgress,
}))
const rewardSummary = computed(() => {
  if (!rewards.value.length) return t('filter.reward')
  const count = progress.value.total
  return rewards.value.map((reward, index) => {
    const source = rawRewards.value[index]
    if (reward.kind === 'orbs' && source.orb_quantity != null) {
      const baseAmount = source.orb_quantity * count
      const premiumAmount = source.premium_orb_quantity != null ? source.premium_orb_quantity * count : null
      const hasPremiumBonus = !!authStore.user?.premium_type
        && premiumAmount != null
        && premiumAmount > baseAmount
      return hasPremiumBonus
        ? `${baseAmount.toLocaleString()} -> ${premiumAmount.toLocaleString()} Orbs`
        : `${baseAmount.toLocaleString()} Orbs`
    }
    if (reward.kind !== 'orbs') {
      return `${((source.quantity ?? 1) * count).toLocaleString()}x ${reward.name}`
    }
    return reward.amountText
  }).join(' · ')
})
const live = computed(() => {
  const active = store.quests.find(quest => quest.id === store.activeQuestId)
  return props.group.quests.some(quest => hasLinkedQuestProgress(quest, active))
})
</script>

<template>
  <div v-if="group.quests.length === 1" class="min-w-0"><slot :quest="first" /></div>
  <section v-else class="quest-group-stack relative isolate min-w-0" :class="compact ? 'mb-2 mt-0' : 'mb-6 mt-14'" :data-quest-group="group.key" :data-group-completed="progress.isCompleted">
    <div class="relative z-10 overflow-hidden rounded-xl border border-border/50 bg-card shadow-sm">
      <div v-if="!compact && heroImage" class="relative h-24 bg-cover bg-center" :style="{ backgroundImage: `url(${heroImage})` }">
        <div class="absolute inset-0 bg-gradient-to-t from-card to-transparent" />
      </div>
      <div :class="compact ? 'space-y-2 p-3' : 'space-y-4 p-4 sm:p-5'">
        <div class="flex min-w-0 items-start gap-3">
          <div class="min-w-0 flex-1 space-y-1.5">
            <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
              <h3 class="min-w-0 break-words font-semibold text-foreground" :class="compact ? 'text-xs' : 'text-base'">{{ first.config.messages.quest_name }}</h3>
              <span class="shrink-0 text-xs font-medium text-primary">{{ t('quest_group.members', { count: compact ? group.quests.length : progress.total }) }}</span>
              <span v-if="!compact && progress.isCompleted" class="text-xs font-medium text-green-600 dark:text-green-400">{{ t('home.completed') }}</span>
            </div>
            <p class="break-words text-xs text-muted-foreground">{{ first.config.messages.game_title }}</p>
            <QuestTaskBadges :quest="first" hide-targets />
          </div>
          <button type="button" class="flex shrink-0 items-center gap-1 rounded-md px-2 py-1.5 text-xs text-muted-foreground transition-colors hover:bg-primary/10 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" :aria-expanded="expanded" :aria-controls="contentId" @click="$emit('update:expanded', !expanded)">
            {{ t(expanded ? 'quest_group.collapse' : 'quest_group.expand') }}
            <ChevronDown class="h-4 w-4 transition-transform motion-reduce:transition-none" :class="expanded && 'rotate-180'" />
          </button>
        </div>
        <div class="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 pt-1" :aria-label="rewardSummary">
          <div v-if="rewards.length" class="flex shrink-0 items-center gap-1.5">
            <div v-for="reward in rewards.slice(0, 3)" :key="rewardKey(reward)" class="flex h-7 w-7 items-center justify-center overflow-hidden rounded-md border border-border/70 bg-muted/70" :class="compact && 'h-6 w-6'">
              <video v-if="reward.asset?.endsWith('.mp4')" :src="`https://cdn.discordapp.com/${reward.asset}`" class="h-full w-full object-contain" autoplay loop muted playsinline />
              <img v-else-if="reward.asset" :src="`https://cdn.discordapp.com/${reward.asset}`" :alt="reward.name" class="h-full w-full object-contain" />
              <img v-else-if="reward.icon === 'orbs'" src="/icons/orbs.png" :alt="reward.name" class="h-5 w-5 object-contain" />
              <Gift v-else class="h-4 w-4 text-pink-400" aria-hidden="true" />
            </div>
          </div>
          <span class="min-w-0 flex-1 truncate text-xs text-muted-foreground">{{ rewardSummary }}</span>
          <span class="ml-auto flex shrink-0 items-center gap-1 text-xs text-muted-foreground">
            <Clock class="h-3 w-3" aria-hidden="true" />
            {{ expirySummary }}
          </span>
        </div>
        <template v-if="!compact">
          <p class="flex flex-wrap gap-x-4 gap-y-1 text-xs tabular-nums text-muted-foreground">
            <span v-if="group.quests.length !== progress.total">{{ t('quest_group.visible_members', { count: group.quests.length, total: progress.total }) }}</span>
            <span>{{ t('quest_group.accepted', { count: stats.accepted, total: progress.total }) }}</span>
            <span>{{ t('quest_group.completed', { count: progress.completed, total: progress.total }) }}</span>
            <span v-if="stats.claimable">{{ t('quest_group.claimable', { count: stats.claimable }) }}</span>
          </p>
          <div class="space-y-2">
            <p class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
              <span>{{ t('quest_group.average_progress') }}</span>
              <span v-if="live" class="inline-flex items-center gap-1 text-primary"><Link2 class="h-3.5 w-3.5" aria-hidden="true" />{{ t('quest_group.linked_progress') }}</span>
              <span class="ml-auto font-mono tabular-nums">{{ Math.round(progress.percentage) }}%</span>
            </p>
            <div class="h-1.5 overflow-hidden rounded-full bg-secondary" role="progressbar" :aria-label="t('quest_group.average_progress')" :aria-valuenow="Math.round(progress.percentage)" :aria-valuemin="0" :aria-valuemax="100"><div class="h-full rounded-full transition-[width] duration-300 motion-reduce:transition-none" :class="progress.isCompleted ? 'bg-green-500' : 'bg-primary'" :style="{ width: `${progress.percentage}%` }" /></div>
          </div>
          <slot name="actions" />
        </template>
      </div>
    </div>
    <div
      v-if="compact"
      :id="contentId"
      class="quest-group-members quest-group-members--compact"
      :inert="!expanded"
      :aria-hidden="expanded ? undefined : 'true'"
      :class="expanded && 'quest-group-members--compact-expanded'"
    >
      <div
        v-for="quest in group.quests"
        :key="quest.id"
        class="quest-group-member"
      >
        <slot :quest="quest" />
      </div>
    </div>
    <TransitionGroup
      v-else
      :id="contentId"
      name="quest-group-fan"
      tag="div"
      class="quest-group-members"
      :inert="!expanded"
      :aria-hidden="expanded ? undefined : 'true'"
      :class="{
        'quest-group-members--expanded': expanded,
        'quest-group-members--collapsed': !expanded,
      }"
    >
      <div
        v-for="(quest, index) in group.quests"
        :key="quest.id"
        class="quest-group-member"
        :class="index >= 3 && 'quest-group-member--stack-hidden'"
        :style="{ '--stack-order': Math.min(index, 2), zIndex: index < 3 ? index + 1 : 0 }"
      >
        <slot :quest="quest" />
      </div>
    </TransitionGroup>
  </section>
</template>

<style scoped>
.quest-group-members {
  position: relative;
  z-index: 0;
  contain: layout;
}

.quest-group-members--compact {
  max-height: 0;
  overflow: hidden;
  padding: 0 1rem;
  opacity: 0;
  transition: max-height 360ms cubic-bezier(0.22, 1, 0.36, 1), padding-top 360ms cubic-bezier(0.22, 1, 0.36, 1), opacity 180ms ease;
}

.quest-group-members--compact-expanded {
  max-height: 1200px;
  padding-top: 0.75rem;
  opacity: 1;
}

.quest-group-members--collapsed {
  position: absolute;
  top: -36px;
  right: 0;
  left: 0;
  height: 36px;
}

.quest-group-member {
  min-width: 0;
  transition: opacity 260ms ease;
}

.quest-group-members--collapsed .quest-group-member--stack-hidden {
  opacity: 0;
  pointer-events: none;
}

.quest-group-members--collapsed .quest-group-member {
  position: absolute;
  top: 0;
  right: calc(var(--stack-order) * 8px);
  left: calc(var(--stack-order) * 8px);
  height: 68px;
  overflow: hidden;
  border: 1px solid hsl(var(--primary) / 0.36);
  border-radius: 1rem;
  background: hsl(var(--card));
  box-shadow: 0 6px 14px -11px hsl(var(--foreground) / 0.5);
  transform: translateY(calc(var(--stack-order) * 12px));
  transform-origin: top center;
}

.quest-group-members--expanded {
  display: grid;
  gap: 0;
  padding: 0.75rem 1rem 0;
}

.quest-group-members--expanded .quest-group-member {
  position: relative;
  top: auto;
  right: auto;
  left: auto;
  height: auto;
  overflow: visible;
  transform: none;
}

.quest-group-fan-move {
  transition: transform 360ms cubic-bezier(0.4, 0, 0.2, 1);
  will-change: transform;
}

@media (prefers-reduced-motion: reduce) {
  .quest-group-members--compact,
  .quest-group-members--collapsed .quest-group-member,
  .quest-group-fan-move {
    transition-duration: 1ms;
  }

  .quest-group-member {
    transition-duration: 1ms;
  }
}
</style>
