<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { ArrowDown, ArrowUp, Check, Gamepad2, Loader2, Plus, RefreshCw, Search, Trash2 } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useGameIdleStore } from '@/stores/gameIdle'
import { useQuestsStore } from '@/stores/quests'
import type { GameIdleCandidateType } from '@/api/tauri'

const { t } = useI18n()
const idle = useGameIdleStore()
const quests = useQuestsStore()
const query = ref('')
const type = ref<GameIdleCandidateType>('all')
const loading = ref(false)
const error = ref('')
const selectedIds = computed(() => new Set(idle.playlist.map(item => item.id)))
const matches = computed(() => idle.detectableGames.filter(game =>
  (type.value === 'all' || game.type_name === (type.value === 'game' ? 'Game' : 'App')) &&
  (game.name.toLocaleLowerCase().includes(query.value.trim().toLocaleLowerCase()) || game.id.includes(query.value.trim()))))
const displayed = computed(() => matches.value.slice(0, 50))
const locked = computed(() => idle.isActive || idle.loading)

async function load(force = false) {
  loading.value = true
  error.value = ''
  try {
    await quests.getDetectableGames(force)
  } catch {
    error.value = t('game_idle.catalog_error')
  } finally {
    loading.value = false
  }
}
onMounted(() => load())
</script>

<template>
  <div class="grid min-w-0 gap-5 lg:grid-cols-2">
    <section class="min-w-0 rounded-lg border bg-background/50 p-4" :aria-label="t('game_idle.catalog')">
      <div class="mb-3 flex items-center justify-between gap-3">
        <h4 class="text-sm font-semibold">{{ t('game_idle.catalog') }}</h4>
        <Button variant="ghost" size="icon" class="h-8 w-8" :aria-label="t('game_idle.refresh_catalog')" :disabled="loading || locked" @click="load(true)">
          <RefreshCw class="h-4 w-4" :class="loading && 'animate-spin'" />
        </Button>
      </div>
      <div class="flex flex-wrap gap-2">
        <div class="relative min-w-32 flex-1">
          <Search class="absolute left-3 top-3 h-4 w-4 text-muted-foreground" aria-hidden="true" />
          <Input v-model="query" class="pl-9" :placeholder="t('game_idle.search_catalog')" :aria-label="t('game_idle.search_catalog')" :disabled="locked" />
        </div>
        <select v-model="type" class="h-10 rounded-md border bg-background px-2 text-xs" :aria-label="t('game_idle.candidate_type')" :disabled="locked">
          <option v-for="choice in (['all', 'game', 'app'] as const)" :key="choice" :value="choice">{{ t(`game_idle.type_${choice}`) }}</option>
        </select>
      </div>
      <p v-if="error" class="mt-3 text-sm text-destructive" role="alert">{{ error }}</p>
      <div v-else-if="loading && !idle.detectableGames.length" class="flex justify-center py-8"><Loader2 class="h-5 w-5 animate-spin" /></div>
      <div v-else class="mt-3 max-h-80 space-y-1 overflow-y-auto">
        <p v-if="!displayed.length" class="py-6 text-center text-sm text-muted-foreground">{{ t('game_sim.no_match') }}</p>
        <button v-for="game in displayed" :key="game.id" type="button" class="flex w-full items-center gap-3 rounded-md px-2 py-2 text-left transition-colors hover:bg-muted focus-visible:outline focus-visible:outline-2 focus-visible:outline-ring disabled:opacity-50" :disabled="locked || selectedIds.has(game.id)" :aria-label="t(selectedIds.has(game.id) ? 'game_idle.already_added' : 'game_idle.add_item', { name: game.name })" @click="idle.addToPlaylist(game)">
          <div class="relative h-9 w-9 shrink-0 overflow-hidden rounded-md bg-muted">
            <Gamepad2 class="absolute inset-2 h-5 w-5 text-muted-foreground" aria-hidden="true" />
            <img v-if="game.icon" :src="`https://cdn.discordapp.com/app-icons/${game.id}/${game.icon}.png?size=64`" class="relative h-full w-full object-cover" alt="" loading="lazy" @error="(event) => (event.target as HTMLImageElement).style.display = 'none'" />
          </div>
          <span class="min-w-0 flex-1">
            <span class="block truncate text-sm font-medium">{{ game.name }}</span>
            <span class="block text-xs text-muted-foreground">{{ t(game.type_name === 'App' ? 'game_idle.type_app' : 'game_idle.type_game') }}<span v-if="!idle.isCompatible(game)" class="text-amber-600 dark:text-amber-300"> · {{ t('game_idle.incompatible') }}</span></span>
          </span>
          <Check v-if="selectedIds.has(game.id)" class="h-4 w-4 shrink-0 text-primary" aria-hidden="true" />
          <Plus v-else class="h-4 w-4 shrink-0" aria-hidden="true" />
        </button>
      </div>
      <p v-if="matches.length > 50" class="mt-2 text-xs text-muted-foreground">{{ t('game_idle.search_limit', { count: 50, total: matches.length }) }}</p>
    </section>

    <section class="min-w-0 rounded-lg border bg-background/50 p-4" :aria-label="t('game_idle.playlist')">
      <h4 class="text-sm font-semibold">{{ t('game_idle.playlist') }} · {{ idle.playlist.length }}</h4>
      <p class="mt-1 text-xs leading-5 text-muted-foreground">{{ t('game_idle.playlist_help') }}</p>
      <p v-if="!idle.playlist.length" class="py-8 text-center text-sm text-muted-foreground">{{ t('game_idle.playlist_empty_error') }}</p>
      <ol v-else class="mt-3 max-h-80 space-y-2 overflow-y-auto">
        <li v-for="(item, index) in idle.playlistEntries" :key="item.id" class="flex items-center gap-2 rounded-md border px-2 py-2" :class="item.unavailable && 'border-destructive/40'">
          <span class="w-5 shrink-0 text-center text-xs tabular-nums text-muted-foreground">{{ index + 1 }}</span>
          <span class="min-w-0 flex-1">
            <span class="block truncate text-sm font-medium">{{ item.name }}</span>
            <span class="block text-xs" :class="item.unavailable ? 'text-destructive' : 'text-muted-foreground'">{{ item.unavailable ? t(`game_idle.${item.unavailable}`) : t(item.type_name === 'App' ? 'game_idle.type_app' : 'game_idle.type_game') }}</span>
          </span>
          <div class="flex shrink-0">
            <Button variant="ghost" size="icon" class="h-8 w-8" :disabled="locked || index === 0" :aria-label="t('game_idle.move_up', { name: item.name })" @click="idle.movePlaylistItem(item.id, -1)"><ArrowUp class="h-4 w-4" /></Button>
            <Button variant="ghost" size="icon" class="h-8 w-8" :disabled="locked || index === idle.playlist.length - 1" :aria-label="t('game_idle.move_down', { name: item.name })" @click="idle.movePlaylistItem(item.id, 1)"><ArrowDown class="h-4 w-4" /></Button>
            <Button variant="ghost" size="icon" class="h-8 w-8 text-destructive" :disabled="locked" :aria-label="t('game_idle.remove_item', { name: item.name })" @click="idle.removeFromPlaylist(item.id)"><Trash2 class="h-4 w-4" /></Button>
          </div>
        </li>
      </ol>
    </section>
  </div>
</template>
