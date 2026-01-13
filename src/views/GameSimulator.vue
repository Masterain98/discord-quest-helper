<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import GameSelector from '@/components/GameSelector.vue'
import type { DetectableGame } from '@/api/tauri'
import { createSimulatedGame, runSimulatedGame, stopSimulatedGame, connectToDiscordRpc } from '@/api/tauri'
import { documentDir, sep } from '@tauri-apps/api/path'
import { emit } from '@tauri-apps/api/event'
import { Card, CardHeader, CardTitle, CardContent, CardDescription, CardFooter } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Loader2, Play, Square, Hammer } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const selectedGame = ref<DetectableGame | null>(null)
const selectedExecutable = ref('')
const installPath = ref('')
const creating = ref(false)
const running = ref(false)
const stopping = ref(false)
const error = ref<string | null>(null)
const success = ref<string | null>(null)

onMounted(async () => {
  const docDir = await documentDir()
  const separator = await sep()
  installPath.value = `${docDir}${separator}DiscordQuestGames`
})

const windowsExecutables = computed(() => {
  if (!selectedGame.value) return []
  return selectedGame.value.executables.filter(e => e.os === 'win32')
})

function selectGame(game: DetectableGame) {
  selectedGame.value = game
  const winExe = game.executables.find(e => e.os === 'win32')
  if (winExe) {
    selectedExecutable.value = winExe.name
  }
  error.value = null
  success.value = null
}

async function handleCreateGame() {
  if (!selectedGame.value || !selectedExecutable.value || !installPath.value) return
  
  creating.value = true
  error.value = null
  success.value = null
  
  try {
    await createSimulatedGame(installPath.value, selectedExecutable.value, selectedGame.value.id)
    success.value = 'Simulated game created successfully!'
  } catch (e) {
    error.value = e as string
  } finally {
    creating.value = false
  }
}

async function handleRunGame() {
  if (!selectedGame.value || !selectedExecutable.value || !installPath.value) return
  
  running.value = true
  error.value = null
  success.value = null
  
  try {
    await runSimulatedGame(
      selectedGame.value.name,
      installPath.value,
      selectedExecutable.value,
      selectedGame.value.id
    )
    
    // Connect RPC
    const activity = {
      app_id: selectedGame.value.id,
      details: "Playing for Discord Quest",
      state: "In Game",
      large_image_key: "logo",
      large_image_text: selectedGame.value.name,
      start_timestamp: Date.now()
    }
    await connectToDiscordRpc(JSON.stringify(activity), 'connect')

    success.value = 'Simulated game is now running with RPC!'
  } catch (e) {
    error.value = e as string
  } finally {
    running.value = false
  }
}

async function handleStopGame() {
  if (!selectedExecutable.value) return
  
  stopping.value = true
  error.value = null
  success.value = null
  
  try {
    await stopSimulatedGame(selectedExecutable.value)
    // Disconnect RPC
    await emit('event_disconnect')
    success.value = 'Simulated game stopped!'
  } catch (e) {
    error.value = e as string
  } finally {
    stopping.value = false
  }
}
</script>

<template>
  <div class="game-simulator-view fade-in space-y-6">
    <div class="flex justify-between items-center">
       <h2 class="text-2xl font-bold tracking-tight">{{ t('game_sim.title') }}</h2>
    </div>
    
    
    <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
      <GameSelector @select="selectGame" />
      
      <Card>
        <CardHeader>
          <CardTitle>{{ t('game_sim.config_title') }}</CardTitle>
          <CardDescription>{{ t('game_sim.config_desc') }}</CardDescription>
        </CardHeader>
        
        <CardContent>
          <div v-if="!selectedGame" class="text-center py-8 text-muted-foreground border-2 border-dashed rounded-lg">
            {{ t('game_sim.select_game') }}
          </div>
          
          <div v-else class="space-y-6">
            <div class="p-4 bg-muted/50 rounded-lg space-y-1">
              <div class="font-bold text-lg text-primary">{{ selectedGame.name }}</div>
              <div class="text-xs text-muted-foreground font-mono">App ID: {{ selectedGame.id }}</div>
            </div>
            
            <div class="space-y-2">
              <Label>{{ t('game_sim.select_exe') }}</Label>
              <select 
                v-model="selectedExecutable"
                class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
              >
                <option 
                  v-for="exe in windowsExecutables" 
                  :key="exe.name"
                  :value="exe.name"
                >
                  {{ exe.name }}
                </option>
              </select>
            </div>
            
            <div class="space-y-2">
              <Label>{{ t('game_sim.install_path') }}</Label>
              <Input 
                v-model="installPath"
                placeholder="C:\Games\MyGame"
              />
            </div>
            
            <div v-if="error" class="p-3 bg-destructive/10 text-destructive rounded-md text-sm">
              {{ error }}
            </div>
            
            <div v-if="success" class="p-3 bg-green-500/10 text-green-600 rounded-md text-sm">
              {{ success }}
            </div>
          </div>
        </CardContent>

        <CardFooter v-if="selectedGame" class="flex flex-col gap-2">
          <Button 
            @click="handleCreateGame"
            class="w-full"
            variant="outline"
            :disabled="!selectedExecutable || !installPath || creating"
          >
            <Hammer v-if="!creating" class="w-4 h-4 mr-2" />
            <Loader2 v-else class="w-4 h-4 mr-2 animate-spin" />
            {{ creating ? t('game_sim.creating') : t('game_sim.create_game') }}
          </Button>
          
          <div class="grid grid-cols-2 gap-2 w-full">
            <Button 
              @click="handleRunGame"
              class="w-full bg-green-600 hover:bg-green-700 text-white"
              :disabled="!selectedExecutable || !installPath || running"
            >
              <Play v-if="!running" class="w-4 h-4 mr-2" />
              <Loader2 v-else class="w-4 h-4 mr-2 animate-spin" />
              {{ running ? t('game_sim.starting') : t('game_sim.run_game') }}
            </Button>
            
            <Button 
              @click="handleStopGame"
              variant="destructive"
              class="w-full"
              :disabled="!selectedExecutable || stopping"
            >
              <Square v-if="!stopping" class="w-4 h-4 mr-2" />
              <Loader2 v-else class="w-4 h-4 mr-2 animate-spin" />
              {{ stopping ? t('game_sim.stopping') : t('game_sim.stop') }}
            </Button>
          </div>
        </CardFooter>
      </Card>
    </div>
  </div>
</template>

