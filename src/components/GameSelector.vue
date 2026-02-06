<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import type { DetectableGame } from '@/api/tauri'
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Input } from '@/components/ui/input'
import { Loader2, Gamepad2, Search, RefreshCw } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { useQuestsStore } from '@/stores/quests'

const { t } = useI18n()
const store = useQuestsStore()

defineEmits<{
  select: [game: DetectableGame]
}>()

const games = ref<DetectableGame[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
const searchQuery = ref('')
const refreshing = ref(false)

// Limit displayed games to avoid performance issues with large lists
const DISPLAY_LIMIT = 50

const filteredGames = computed(() => {
  if (!searchQuery.value) {
    return games.value.slice(0, DISPLAY_LIMIT)
  }
  
  const query = searchQuery.value.toLowerCase()
  return games.value
    .filter(g => g.name.toLowerCase().includes(query))
    .slice(0, DISPLAY_LIMIT)
})

async function loadGames(force = false) {
  loading.value = true
  if (force) refreshing.value = true
  error.value = null
  try {
    games.value = await store.getDetectableGames(force)
  } catch (e) {
    error.value = e as string
  } finally {
    loading.value = false
    refreshing.value = false
  }
}

onMounted(async () => {
  await loadGames()
})
</script>

<template>
  <Card class="bg-card flex flex-col h-[600px]">
    <CardHeader>
       <CardTitle class="text-lg flex items-center justify-between gap-2">
        <div class="flex items-center gap-2">
          <Gamepad2 class="w-5 h-5"/>
          {{ t('game_sim.select_title') }}
        </div>
        <Button 
          variant="ghost" 
          size="icon" 
          class="h-8 w-8" 
          @click="loadGames(true)"
          :disabled="loading"
        >
          <RefreshCw class="h-4 w-4" :class="{ 'animate-spin': refreshing }" />
        </Button>
      </CardTitle>
      <div class="relative mt-2">
        <Search class="absolute left-2 top-2.5 h-4 w-4 text-muted-foreground" />
        <Input 
          v-model="searchQuery" 
          :placeholder="t('game_sim.search')" 
          class="pl-8"
        />
      </div>
    </CardHeader>
    
    <CardContent class="flex-1 overflow-hidden p-0 px-6 pb-6">
      <div v-if="loading && games.length === 0" class="flex justify-center py-8">
        <Loader2 class="w-6 h-6 animate-spin text-muted-foreground" />
      </div>
      
      <div v-else-if="error" class="p-3 bg-destructive/10 text-destructive rounded-md text-sm">
        Error: {{ error }}
      </div>
      
      <div v-else-if="games.length === 0" class="text-center py-8 text-muted-foreground">
        No detectable games found.
      </div>
      
      <div v-else-if="filteredGames.length === 0" class="text-center py-8 text-muted-foreground">
        No games match your search.
      </div>

      <div v-else class="space-y-2 h-full overflow-y-auto pr-2">
        <Button
          v-for="game in filteredGames" 
          :key="game.id"
          variant="secondary"
          class="w-full justify-start h-auto py-3 px-4 items-center gap-3 shrink-0"
          @click="$emit('select', game)"
        >
          <!-- Icon -->
          <div class="relative w-10 h-10 rounded-md overflow-hidden bg-muted/50 shrink-0">
             <img 
               v-if="game.icon"
               :src="`https://cdn.discordapp.com/app-icons/${game.id}/${game.icon}.png?size=64`"
               loading="lazy"
               class="w-full h-full object-cover"
               @error="(e) => (e.target as HTMLImageElement).style.display = 'none'"
             />
             <Gamepad2 v-else class="w-5 h-5 absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 text-muted-foreground/50" />
          </div>

          <div class="flex flex-col items-start gap-1 flex-1 min-w-0">
            <div class="flex items-center gap-2 w-full">
               <span class="font-bold truncate">{{ game.name }}</span>
               <Badge variant="outline" class="text-[10px] h-5 px-1.5 shrink-0" :class="game.type_name === 'Game' ? 'border-primary/50 text-primary' : 'border-muted-foreground/50 text-muted-foreground'">
                 {{ game.type_name || 'Game' }}
               </Badge>
            </div>
            <div class="text-xs text-muted-foreground font-normal">
              {{ t('game_sim.exe_count', { count: game.executables.filter(e => e.os === 'win32').length }) }}
            </div>
          </div>
        </Button>
        <div v-if="filteredGames.length < games.length && !loading && !searchQuery" class="text-center text-xs text-muted-foreground pt-4">
           {{ t('game_sim.showing_top', { count: DISPLAY_LIMIT }) }}
        </div>
      </div>
    </CardContent>
  </Card>
</template>


