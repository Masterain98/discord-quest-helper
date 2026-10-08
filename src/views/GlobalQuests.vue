<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { onClickOutside, useNow } from '@vueuse/core'
import { nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import { open } from '@tauri-apps/plugin-shell'
import { Check, CheckCircle2, ChevronDown, ExternalLink, Filter, Globe2, ImageOff, RotateCw, Search } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import GlobalQuestRegions from '@/components/GlobalQuestRegions.vue'
import GlobalQuestTaskBadges from '@/components/GlobalQuestTaskBadges.vue'
import { Input } from '@/components/ui/input'
import { useGlobalQuestsStore } from '@/stores/globalQuests'
import { useAuthStore } from '@/stores/auth'
import { useQuestsStore } from '@/stores/quests'
import { acceptedGlobalQuestIds, completedGlobalQuestIds, filterGlobalQuests, unknownRestriction } from '@/utils/globalQuests'
import type { GlobalQuest, GlobalQuestReward } from '@/types/globalQuests'
import { GLOBAL_COUNTRY_CODES } from '@/utils/globalCountries'
import { navigateDiscordSpa } from '@/api/tauri'
import { openGlobalQuest } from '@/utils/globalQuestNavigation'

const { t, locale } = useI18n()
const store = useGlobalQuestsStore()
const auth = useAuthStore()
const accountQuests = useQuestsStore()
const now = useNow({ interval: 1_000 })
const expanded = ref(new Set<string>())
const failedImages = ref(new Set<string>())
const countryQuery = ref('')
const countryPickerOpen = ref(false)
const countryPicker = ref<HTMLElement | null>(null)
const countryTrigger = ref<HTMLButtonElement | null>(null)
const countrySearchInput = ref<HTMLInputElement | null>(null)
const countryOptions = ref<HTMLElement | null>(null)
const linkError = ref(false)
const openingQuest = ref<string | null>(null)
const completed = computed(() => completedGlobalQuestIds(accountQuests.quests, auth.user?.id ?? null, accountQuests.questAccountId))
const accepted = computed(() => acceptedGlobalQuestIds(accountQuests.quests, auth.user?.id ?? null, accountQuests.questAccountId))
const visible = computed(() => filterGlobalQuests(store.quests, store.restrictions, store.filters, now.value.getTime(), completed.value, accepted.value))
const total = computed(() => store.quests.filter(quest => quest.expiresAt && Date.parse(quest.expiresAt) > now.value.getTime()).length)
const filterCount = computed(() => store.filters.tasks.length + store.filters.rewards.length
  + store.filters.regions.length + store.filters.ages.length + Number(Boolean(store.filters.country)))
const groups = [
  { key: 'tasks' as const, options: ['watch', 'play', 'stream', 'activity', 'other'] as const },
  { key: 'rewards' as const, options: ['orbs', 'avatar', 'ingame', 'nitro', 'other'] as const },
  { key: 'regions' as const, options: ['global', 'restricted', 'unknown'] as const },
  { key: 'ages' as const, options: ['adult', 'unmarked', 'unknown'] as const },
]
// Construct once per locale, rather than once per row or timer tick.
const regionNames = computed(() => new Intl.DisplayNames([locale.value], { type: 'region' }))
function countryName(code: string) {
  const displayName = regionNames.value.of(code)
  const fallback = code === 'EH' && (!displayName || displayName === code)
    ? (locale.value.startsWith('zh') ? '西撒哈拉' : 'Western Sahara')
    : displayName ?? code
  return `${fallback} (${code})`
}
const countries = computed(() => {
  const codes = new Set([...GLOBAL_COUNTRY_CODES, ...Object.values(store.restrictions).flatMap(rule => [...rule.include, ...rule.exclude])])
  if (store.filters.country) codes.add(store.filters.country)
  return [...codes].map(code => ({ code, name: countryName(code) }))
    .sort((a, b) => a.name.localeCompare(b.name, locale.value))
})
const selectedCountry = computed(() => countries.value.find(country => country.code === store.filters.country) ?? null)
const matchingCountries = computed(() => countries.value.filter(country => country.code === store.filters.country
  || country.name.toLocaleLowerCase().includes(countryQuery.value.trim().toLocaleLowerCase())))
onClickOutside(countryPicker, () => { countryPickerOpen.value = false })

function toggleCountryPicker() {
  countryPickerOpen.value = !countryPickerOpen.value
  if (countryPickerOpen.value) nextTick(() => countrySearchInput.value?.focus())
}
function closeCountryPicker() {
  countryPickerOpen.value = false
  nextTick(() => countryTrigger.value?.focus())
}
function chooseCountry(code: string) {
  store.filters.country = code
  countryQuery.value = ''
  countryPickerOpen.value = false
  nextTick(() => countryTrigger.value?.focus())
}
function handleCountrySearchKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault()
    closeCountryPicker()
  } else if (event.key === 'ArrowDown') {
    event.preventDefault()
    countryOptions.value?.querySelector<HTMLButtonElement>('[role="option"]')?.focus()
  }
}
function handleCountryOptionKeydown(event: KeyboardEvent) {
  const options = [...(countryOptions.value?.querySelectorAll<HTMLButtonElement>('[role="option"]') ?? [])]
  const index = options.indexOf(event.currentTarget as HTMLButtonElement)
  if (event.key === 'ArrowUp' && index === 0) {
    event.preventDefault()
    countrySearchInput.value?.focus()
    return
  }
  const nextIndex = event.key === 'ArrowDown' ? Math.min(options.length - 1, index + 1)
    : event.key === 'ArrowUp' ? Math.max(0, index - 1) : -1
  if (nextIndex >= 0) {
    event.preventDefault()
    options[nextIndex]?.focus()
  } else if (event.key === 'Escape') {
    event.preventDefault()
    closeCountryPicker()
  }
}

function toggleFilter(group: typeof groups[number]['key'], value: string) {
  const values = store.filters[group] as string[]
  const index = values.indexOf(value)
  if (index === -1) values.push(value)
  else values.splice(index, 1)
}
function clearFilters() { store.clearFilters(); countryQuery.value = ''; countryPickerOpen.value = false }
function restriction(quest: GlobalQuest) { return store.restrictions[quest.id] ?? unknownRestriction() }
function rewardLabel(reward: GlobalQuestReward) {
  const name = reward.name || t(`global_quests.rewards.${reward.kind}`)
  return reward.quantity !== null && reward.kind !== 'orbs' ? `${name} ×${reward.quantity}`
    : reward.kind === 'orbs' && reward.quantity !== null ? `${reward.quantity.toLocaleString(locale.value)} Orbs` : name
}
function hasRewardDetails(reward: GlobalQuestReward) {
  return reward.premiumQuantity !== null || Boolean(reward.expiresAt) || reward.instructions.length > 0
}
function dateLabel(value: string | null) {
  return value ? new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(value)) : t('global_quests.unknown')
}
function toggleDetails(id: string) { if (expanded.value.has(id)) expanded.value.delete(id); else expanded.value.add(id) }
async function openQuest(id: string) {
  if (openingQuest.value) return
  openingQuest.value = id
  linkError.value = false
  try {
    await openGlobalQuest(id, {
      mode: accountQuests.gameQuestMode, cdpPort: accountQuests.cdpPort,
      navigate: navigateDiscordSpa, openExternal: openExternal,
    })
  } catch { linkError.value = true }
  finally { openingQuest.value = null }
}
async function openSource() { await openExternal('https://discordquest.com/') }
async function openExternal(url: string) {
  linkError.value = false
  try {
    if ('__TAURI_INTERNALS__' in window) await open(url)
    else window.open(url, '_blank', 'noopener,noreferrer')
  } catch { linkError.value = true }
}
async function refresh() {
  await Promise.all([store.refresh(true), auth.user ? accountQuests.fetchQuests(true, true) : Promise.resolve()])
}
watch(() => auth.user?.id, id => { if (id) void accountQuests.fetchQuests(true, true) }, { immediate: true })
void store.refresh()
</script>

<template>
  <div class="space-y-5 pb-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div class="min-w-0 space-y-1.5">
        <h2 class="flex items-center gap-2 text-2xl font-semibold tracking-tight"><Globe2 class="h-6 w-6 text-primary" />{{ t('nav.global_quests') }}</h2>
        <p class="max-w-3xl text-sm leading-relaxed text-muted-foreground">{{ t('global_quests.description') }}</p>
      </div>
      <button type="button" class="inline-flex items-center gap-1.5 text-xs text-muted-foreground hover:text-primary" @click="openSource">
        {{ t('global_quests.source_label') }} <ExternalLink class="h-3 w-3" />
      </button>
    </div>

    <div class="rounded-xl border border-border/70 bg-card/70 p-3.5">
      <div class="flex flex-wrap items-center gap-2">
        <div class="relative min-w-48 flex-1">
          <Search class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
          <Input v-model="store.filters.query" :aria-label="t('global_quests.search')" :placeholder="t('global_quests.search')" class="pl-9" />
        </div>
        <Button variant="outline" class="gap-2" :aria-expanded="store.showFilters" aria-controls="global-quest-filters" @click="store.showFilters = !store.showFilters">
          <Filter class="h-4 w-4" />{{ t('global_quests.filters') }}<Badge v-if="filterCount" variant="secondary">{{ filterCount }}</Badge>
        </Button>
        <Button variant="outline" class="gap-2" :disabled="store.loading || accountQuests.refreshing" @click="refresh">
          <RotateCw class="h-4 w-4" :class="store.loading && 'animate-spin'" />{{ t('general.refresh') }}
        </Button>
      </div>
      <div v-if="store.showFilters" id="global-quest-filters" class="mt-4 space-y-4 border-t border-border pt-4">
        <div class="grid grid-cols-2 gap-5 lg:grid-cols-4">
          <fieldset v-for="group in groups" :key="group.key" class="min-w-0 space-y-2">
            <legend class="mb-2 text-sm font-semibold">{{ t(`global_quests.filter_titles.${group.key}`) }}</legend>
            <label v-for="option in group.options" :key="option" class="flex cursor-pointer items-center gap-2 text-sm">
              <input type="checkbox" class="h-4 w-4 shrink-0 accent-primary" :checked="(store.filters[group.key] as string[]).includes(option)" @change="toggleFilter(group.key, option)">
              {{ t(`global_quests.${group.key}.${option}`) }}
            </label>
          </fieldset>
        </div>
        <div class="flex flex-wrap items-end gap-3 border-t border-border pt-3">
          <div ref="countryPicker" class="relative min-w-60 flex-1 space-y-1.5">
            <label id="global-country-label" class="text-sm font-medium">{{ t('global_quests.country') }}</label>
            <button
              type="button"
              ref="countryTrigger"
              class="inline-flex h-10 w-full items-center justify-between rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50"
              role="combobox"
              aria-haspopup="listbox"
              aria-controls="global-country-options"
              :aria-expanded="countryPickerOpen"
              aria-labelledby="global-country-label"
              @click="toggleCountryPicker"
              @keydown.esc="closeCountryPicker"
            >
              <span class="flex min-w-0 items-center gap-2 truncate">
                <Globe2 v-if="!selectedCountry" class="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden="true" />
                <img v-else :src="`/flags/${selectedCountry.code}.svg`" alt="" width="21" height="14" class="h-3.5 w-[21px] shrink-0 rounded-[2px] ring-1 ring-border/50">
                <span class="truncate">{{ selectedCountry?.name ?? t('global_quests.all_countries') }}</span>
              </span>
              <ChevronDown class="ml-2 h-4 w-4 shrink-0 opacity-50" aria-hidden="true" />
            </button>
            <div v-if="countryPickerOpen" class="absolute left-0 top-full z-50 mt-1 w-full overflow-hidden rounded-md border border-border bg-popover text-popover-foreground shadow-lg ring-1 ring-black/5">
              <div class="border-b border-border p-2">
                <div class="relative">
                  <Search class="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
                  <input
                    id="global-country-search"
                    ref="countrySearchInput"
                    v-model="countryQuery"
                    type="search"
                    role="searchbox"
                    :aria-label="t('global_quests.country_search')"
                    :placeholder="t('global_quests.country_search')"
                    class="h-9 w-full rounded-sm border border-input bg-background pl-9 pr-3 text-sm outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
                    @keydown="handleCountrySearchKeydown"
                  >
                </div>
              </div>
              <div id="global-country-options" ref="countryOptions" role="listbox" aria-labelledby="global-country-label" class="max-h-60 overflow-y-auto p-1">
                <button
                  type="button"
                  role="option"
                  :aria-selected="store.filters.country === ''"
                  class="flex h-9 w-full items-center gap-2 rounded-sm px-2 text-left text-sm outline-none hover:bg-accent hover:text-accent-foreground focus:bg-accent focus:text-accent-foreground"
                  @click="chooseCountry('')"
                  @keydown="handleCountryOptionKeydown"
                >
                  <Globe2 class="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden="true" />
                  <span class="flex-1 truncate">{{ t('global_quests.all_countries') }}</span>
                  <Check v-if="store.filters.country === ''" class="h-4 w-4 shrink-0" aria-hidden="true" />
                </button>
                <button
                  v-for="country in matchingCountries"
                  :key="country.code"
                  type="button"
                  role="option"
                  :aria-selected="store.filters.country === country.code"
                  class="flex h-9 w-full items-center gap-2 rounded-sm px-2 text-left text-sm outline-none hover:bg-accent hover:text-accent-foreground focus:bg-accent focus:text-accent-foreground"
                  @click="chooseCountry(country.code)"
                  @keydown="handleCountryOptionKeydown"
                >
                  <img :src="`/flags/${country.code}.svg`" alt="" width="21" height="14" class="h-3.5 w-[21px] shrink-0 rounded-[2px] ring-1 ring-border/50">
                  <span class="flex-1 truncate">{{ country.name }}</span>
                  <Check v-if="store.filters.country === country.code" class="h-4 w-4 shrink-0" aria-hidden="true" />
                </button>
                <p v-if="!matchingCountries.length" class="px-2 py-3 text-center text-sm text-muted-foreground">{{ t('global_quests.country_empty') }}</p>
              </div>
            </div>
          </div>
          <Button variant="ghost" @click="clearFilters">{{ t('global_quests.clear') }}</Button>
        </div>
        <p class="text-xs text-muted-foreground">{{ t('global_quests.country_hint') }}</p>
      </div>
    </div>

    <div class="flex flex-wrap justify-between gap-2 text-xs text-muted-foreground" aria-live="polite">
      <span>{{ t('global_quests.results', { count: visible.length, total }) }}</span>
      <span v-if="store.updatedAt">{{ t('global_quests.updated', { time: dateLabel(new Date(store.updatedAt).toISOString()) }) }}</span>
    </div>
    <p v-if="store.catalogError && store.hasLoaded" role="alert" class="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm">{{ t('global_quests.refresh_error') }}</p>
    <p v-if="store.regionsError" role="alert" class="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm">{{ t(store.regionsUpdatedAt ? 'global_quests.regions_stale' : 'global_quests.regions_error') }}</p>
    <p v-if="auth.user && accountQuests.error" role="alert" class="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm">{{ t('global_quests.account_error') }}</p>
    <p v-if="linkError" role="alert" class="text-sm text-destructive">{{ t('global_quests.link_error') }}</p>

    <div v-if="!store.hasLoaded" class="rounded-xl border border-border py-16 text-center" role="status">
      <template v-if="store.loading"><RotateCw class="mx-auto mb-3 h-6 w-6 animate-spin text-primary" />{{ t('global_quests.loading') }}</template>
      <template v-else><p class="mb-4 text-muted-foreground">{{ t('global_quests.load_error') }}</p><Button variant="outline" @click="refresh">{{ t('global_quests.retry') }}</Button></template>
    </div>
    <div v-else-if="!visible.length" class="rounded-xl border border-border py-16 text-center">
      <Globe2 class="mx-auto mb-3 h-8 w-8 text-muted-foreground" /><p class="text-muted-foreground">{{ t('global_quests.empty') }}</p>
      <Button v-if="filterCount || store.filters.query" variant="ghost" class="mt-3" @click="clearFilters">{{ t('global_quests.clear') }}</Button>
    </div>
    <ul v-else class="space-y-1.5" :aria-label="t('nav.global_quests')">
      <li v-for="quest in visible" :key="quest.id" class="overflow-hidden rounded-xl border border-border/70 bg-card/70" :data-quest-id="quest.id" :data-completed="completed.has(quest.id)" :data-accepted="accepted.has(quest.id)">
        <div class="global-quest-row" :class="completed.has(quest.id) && 'global-quest-row--completed'">
          <div class="flex h-10 w-14 shrink-0 items-center justify-center overflow-hidden rounded bg-muted">
            <img v-if="quest.thumbnailUrl && !failedImages.has(quest.id)" :src="quest.thumbnailUrl" alt="" loading="lazy" class="h-full w-full object-cover" @error="failedImages.add(quest.id)">
            <ImageOff v-else class="h-5 w-5 text-muted-foreground" />
          </div>
          <div class="min-w-0 space-y-1">
            <div class="flex items-center gap-1.5">
              <h3 class="truncate text-sm font-semibold leading-snug" :title="quest.name">{{ quest.name }}</h3>
              <Badge v-if="accepted.has(quest.id)" variant="outline" class="shrink-0 gap-1 border-sky-500/30 bg-sky-500/10 px-1.5 py-0 text-[10px] text-sky-700 dark:text-sky-400"><Check class="h-3 w-3" />{{ t('global_quests.accepted') }}</Badge>
              <Badge v-if="completed.has(quest.id)" variant="outline" class="global-completed-badge shrink-0 gap-1 border-green-500/30 bg-green-500/10 px-1.5 py-0 text-[10px] text-green-700 dark:text-green-400"><CheckCircle2 class="h-3 w-3" />{{ t('global_quests.completed') }}</Badge>
              <Badge v-if="quest.startsAt && Date.parse(quest.startsAt) > now.getTime()" variant="outline">{{ t('global_quests.upcoming') }}</Badge>
            </div>
            <div class="flex flex-wrap gap-1">
              <GlobalQuestTaskBadges :tasks="quest.tasks" compact />
              <Badge v-if="!quest.tasks.length" variant="secondary" class="px-1.5 py-0 text-[10px] font-normal leading-4">{{ t('global_quests.tasks.other') }}</Badge>
              <Badge v-if="!quest.tasks.length" variant="secondary">{{ t('global_quests.tasks.other') }}</Badge>
            </div>
          </div>
          <div class="min-w-0 space-y-1 text-xs">
            <p v-for="(reward, index) in quest.rewards" :key="index" class="break-words">{{ rewardLabel(reward) }}</p>
            <p v-if="!quest.rewards.length" class="text-muted-foreground">{{ t('global_quests.rewards.other') }}</p>
            <p class="text-xs text-muted-foreground">{{ t('global_quests.ends', { time: dateLabel(quest.expiresAt) }) }}</p>
          </div>
          <div class="flex min-w-0 flex-wrap items-center gap-1.5">
            <GlobalQuestRegions :restriction="restriction(quest)" />
            <Badge v-if="restriction(quest).age === 'adult'" variant="outline" class="border-amber-500/40 px-1.5 py-0 text-[10px] text-amber-700 dark:text-amber-300">18+</Badge>
          </div>
          <Button variant="ghost" size="icon" class="h-8 w-8 shrink-0" :title="t('global_quests.details')" :aria-expanded="expanded.has(quest.id)" :aria-controls="`global-details-${quest.id}`" :aria-label="`${t('global_quests.details')}: ${quest.name}`" @click="toggleDetails(quest.id)">
            <ChevronDown class="h-4 w-4 transition-transform" :class="expanded.has(quest.id) && 'rotate-180'" />
          </Button>
        </div>
        <div v-if="expanded.has(quest.id)" :id="`global-details-${quest.id}`" class="space-y-4 border-t border-border bg-background/40 p-4 text-sm">
          <div class="grid gap-5 md:grid-cols-2">
            <div v-if="quest.description || quest.startsAt" class="space-y-2">
              <p v-if="quest.description" class="whitespace-pre-line break-words text-muted-foreground">{{ quest.description }}</p>
              <p v-if="quest.startsAt">{{ t('global_quests.starts', { time: dateLabel(quest.startsAt) }) }}</p>
            </div>
            <div v-if="restriction(quest).region === 'restricted' || restriction(quest).age === 'adult'" class="space-y-2">
              <h4 class="font-semibold">{{ t('global_quests.restrictions') }}</h4>
              <p v-if="restriction(quest).region === 'restricted' && restriction(quest).include.length">{{ t('global_quests.only_countries', { countries: restriction(quest).include.map(countryName).join(', ') }) }}</p>
              <p v-if="restriction(quest).region === 'restricted' && restriction(quest).exclude.length">{{ t('global_quests.except_countries', { countries: restriction(quest).exclude.map(countryName).join(', ') }) }}</p>
              <p v-if="restriction(quest).age === 'adult'" class="text-xs text-muted-foreground">{{ t('global_quests.age_hint') }}</p>
              <p class="text-xs text-muted-foreground">{{ t('global_quests.eligibility_hint') }}</p>
            </div>
          </div>
          <div v-for="(reward, index) in quest.rewards.filter(hasRewardDetails)" :key="index" class="space-y-1.5 border-t border-border pt-3">
              <p class="font-medium">{{ rewardLabel(reward) }}</p>
              <p v-if="reward.premiumQuantity !== null" class="text-xs text-muted-foreground">{{ t('global_quests.premium_reward', { count: reward.premiumQuantity }) }}</p>
              <p v-if="reward.expiresAt" class="text-xs text-muted-foreground">{{ t('global_quests.reward_expires', { time: dateLabel(reward.expiresAt) }) }}</p>
              <p v-for="instruction in reward.instructions" :key="instruction" class="whitespace-pre-line break-words text-xs leading-relaxed text-muted-foreground">{{ instruction }}</p>
          </div>
          <div class="flex flex-wrap items-center justify-between gap-2 border-t border-border pt-3">
            <span class="text-xs text-muted-foreground">ID: {{ quest.id }}</span>
            <Button variant="outline" class="gap-2" :disabled="openingQuest !== null" @click="openQuest(quest.id)">{{ t('global_quests.open_discord') }}<RotateCw v-if="openingQuest === quest.id" class="h-4 w-4 animate-spin" /><ExternalLink v-else class="h-4 w-4" /></Button>
          </div>
        </div>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.global-quest-row {
  display: grid;
  grid-template-columns: 3.5rem minmax(0, 1.7fr) minmax(0, 1fr) minmax(5.5rem, auto) 2rem;
  align-items: center;
  gap: 0.75rem;
  padding: 0.625rem 0.75rem;
}
.global-quest-row--completed { background: hsl(var(--muted) / 0.45); color: hsl(var(--muted-foreground)); }
.global-quest-row--completed img { opacity: 0.6; }
@media (max-width: 639px) {
  .global-quest-row { grid-template-columns: 3.5rem minmax(0, 1fr) 2rem; }
  .global-quest-row > :nth-child(3) { grid-column: 2; }
  .global-quest-row > :nth-child(4) { grid-column: 2; }
  .global-quest-row > :last-child { grid-column: 3; grid-row: 1; }
}
@media (prefers-reduced-motion: reduce) { .transition-transform { transition: none; } }
</style>
