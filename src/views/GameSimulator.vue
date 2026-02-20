<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import GameSelector from '@/components/GameSelector.vue'
import type { DetectableGame } from '@/api/tauri'
import { createSimulatedGame, runSimulatedGame, connectToDiscordRpc } from '@/api/tauri'
import { documentDir, sep } from '@tauri-apps/api/path'
import { open as openFolderPicker } from '@tauri-apps/plugin-dialog'
import { Card, CardHeader, CardTitle, CardContent, CardDescription, CardFooter } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from '@/components/ui/dialog'
import { Loader2, Play, Hammer, List, Terminal, FolderOpen } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

// Mode: 'select' = pick from detectable games list, 'custom' = enter any process name
const mode = ref<'select' | 'custom'>('select')

const selectedGame = ref<DetectableGame | null>(null)
const selectedExecutable = ref('')
const customExeName = ref('')
const installPath = ref('')
const running = ref(false)
const creating = ref(false)
const error = ref<string | null>(null)
const success = ref<string | null>(null)

// Create dialog state
const showCreateDialog = ref(false)
const dialogSavePath = ref('')

onMounted(async () => {
  const docDir = await documentDir()
  const separator = await sep()
  installPath.value = `${docDir}${separator}DiscordQuestGames`
})

const windowsExecutables = computed(() => {
  if (!selectedGame.value) return []
  return selectedGame.value.executables.filter(e => e.os === 'win32')
})

const hasWindowsExecutables = computed(() => windowsExecutables.value.length > 0)

// The executable name that will actually be used for run/create
const effectiveExecutable = computed(() =>
  mode.value === 'custom' ? customExeName.value : selectedExecutable.value
)

// Whether the footer action buttons should be shown
const canProceed = computed(() => {
  if (mode.value === 'custom') return !!customExeName.value
  return !!selectedGame.value && hasWindowsExecutables.value && !!selectedExecutable.value
})

function switchMode(m: 'select' | 'custom') {
  mode.value = m
  error.value = null
  success.value = null
}

function selectGame(game: DetectableGame) {
  selectedGame.value = game
  const winExe = game.executables.find(e => e.os === 'win32')
  selectedExecutable.value = winExe ? winExe.name : ''
  error.value = null
  success.value = null
}

function openCreateDialog() {
  dialogSavePath.value = installPath.value
  showCreateDialog.value = true
}

async function pickInstallFolder() {
  const selected = await openFolderPicker({ directory: true, multiple: false, defaultPath: installPath.value || undefined })
  if (typeof selected === 'string') installPath.value = selected
}

async function pickDialogFolder() {
  const selected = await openFolderPicker({ directory: true, multiple: false, defaultPath: dialogSavePath.value || undefined })
  if (typeof selected === 'string') dialogSavePath.value = selected
}

async function handleCreateGame() {
  if (!effectiveExecutable.value || !dialogSavePath.value) return

  creating.value = true
  error.value = null
  success.value = null

  try {
    const appId = mode.value === 'custom' ? '' : (selectedGame.value?.id ?? '')
    await createSimulatedGame(dialogSavePath.value, effectiveExecutable.value, appId)
    showCreateDialog.value = false
    success.value = t('game_sim.create_success')
  } catch (e) {
    error.value = e as string
  } finally {
    creating.value = false
  }
}

async function handleRunGame() {
  if (!effectiveExecutable.value || !installPath.value) return

  running.value = true
  error.value = null
  success.value = null

  try {
    const appId = mode.value === 'custom' ? '' : (selectedGame.value?.id ?? '')
    const displayName = mode.value === 'custom' ? customExeName.value : (selectedGame.value?.name ?? '')
    await runSimulatedGame(displayName, installPath.value, effectiveExecutable.value, appId)

    if (mode.value === 'select' && selectedGame.value) {
      const activity = {
        app_id: selectedGame.value.id,
        details: 'Playing for Discord Quest',
        state: 'In Game',
        large_image_key: 'logo',
        large_image_text: selectedGame.value.name,
        start_timestamp: Date.now()
      }
      await connectToDiscordRpc(JSON.stringify(activity), 'connect')
      success.value = t('game_sim.run_success_rpc')
    } else {
      success.value = t('game_sim.run_success')
    }
  } catch (e) {
    error.value = e as string
  } finally {
    running.value = false
  }
}
</script>

<template>
  <div class="game-simulator-view fade-in space-y-6">
    <div class="flex justify-between items-center flex-wrap gap-3">
      <h2 class="text-2xl font-bold tracking-tight">{{ t('game_sim.title') }}</h2>
      <!-- Mode toggle -->
      <div class="flex rounded-lg border p-1 gap-1 bg-muted/50">
        <Button
          size="sm"
          :variant="mode === 'select' ? 'default' : 'ghost'"
          class="gap-1.5 h-7 px-3 text-xs"
          @click="switchMode('select')"
        >
          <List class="w-3.5 h-3.5" />
          {{ t('game_sim.mode_from_list') }}
        </Button>
        <Button
          size="sm"
          :variant="mode === 'custom' ? 'default' : 'ghost'"
          class="gap-1.5 h-7 px-3 text-xs"
          @click="switchMode('custom')"
        >
          <Terminal class="w-3.5 h-3.5" />
          {{ t('game_sim.mode_custom') }}
        </Button>
      </div>
    </div>

    <div class="grid grid-cols-1 gap-6" :class="mode === 'select' ? 'lg:grid-cols-2' : ''">
      <GameSelector v-if="mode === 'select'" @select="selectGame" />

      <Card>
        <CardHeader>
          <CardTitle>{{ t('game_sim.config_title') }}</CardTitle>
          <CardDescription>{{ mode === 'custom' ? t('game_sim.custom_config_desc') : t('game_sim.config_desc') }}</CardDescription>
        </CardHeader>

        <CardContent>
          <!-- ── SELECT MODE ─────────────────────────── -->
          <template v-if="mode === 'select'">
            <div v-if="!selectedGame" class="text-center py-8 text-muted-foreground border-2 border-dashed rounded-lg">
              {{ t('game_sim.select_game') }}
            </div>

            <div v-else class="space-y-6">
              <div class="p-4 bg-muted/50 rounded-lg space-y-1">
                <div class="font-bold text-lg text-primary">{{ selectedGame.name }}</div>
                <div class="text-xs text-muted-foreground font-mono">App ID: {{ selectedGame.id }}</div>
              </div>

              <div v-if="!hasWindowsExecutables" class="p-3 bg-yellow-500/10 text-yellow-600 dark:text-yellow-400 rounded-md text-sm border border-yellow-500/20">
                {{ t('game_sim.no_exe_warning') }}
              </div>

              <template v-else>
                <div class="space-y-2">
                  <Label>{{ t('game_sim.select_exe') }}</Label>
                  <select
                    v-model="selectedExecutable"
                    class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    <option v-for="exe in windowsExecutables" :key="exe.name" :value="exe.name">
                      {{ exe.name }}
                    </option>
                  </select>
                </div>

                <div class="space-y-2">
                  <Label>{{ t('game_sim.install_path') }}</Label>
                  <div class="flex gap-2">
                    <Input v-model="installPath" placeholder="C:\Games\MyGame" class="flex-1" />
                    <Button type="button" variant="outline" size="icon" @click="pickInstallFolder" class="shrink-0">
                      <FolderOpen class="w-4 h-4" />
                    </Button>
                  </div>
                </div>
              </template>

              <div v-if="error" class="p-3 bg-destructive/10 text-destructive rounded-md text-sm">{{ error }}</div>
              <div v-if="success" class="p-3 bg-green-500/10 text-green-600 rounded-md text-sm">{{ success }}</div>
            </div>
          </template>

          <!-- ── CUSTOM MODE ─────────────────────────── -->
          <template v-else>
            <div class="space-y-6">
              <div class="space-y-2">
                <Label>{{ t('game_sim.custom_exe_label') }}</Label>
                <Input
                  v-model="customExeName"
                  :placeholder="t('game_sim.custom_exe_placeholder')"
                />
                <p class="text-xs text-muted-foreground">{{ t('game_sim.custom_exe_hint') }}</p>
              </div>

              <div class="space-y-2">
                <Label>{{ t('game_sim.install_path') }}</Label>
                <div class="flex gap-2">
                  <Input v-model="installPath" placeholder="C:\Games\MyGame" class="flex-1" />
                  <Button type="button" variant="outline" size="icon" @click="pickInstallFolder" class="shrink-0">
                    <FolderOpen class="w-4 h-4" />
                  </Button>
                </div>
              </div>

              <div v-if="error" class="p-3 bg-destructive/10 text-destructive rounded-md text-sm">{{ error }}</div>
              <div v-if="success" class="p-3 bg-green-500/10 text-green-600 rounded-md text-sm">{{ success }}</div>
            </div>
          </template>
        </CardContent>

        <CardFooter v-if="canProceed" class="flex flex-col gap-2">
          <div class="grid grid-cols-2 gap-2 w-full">
            <Button
              @click="handleRunGame"
              class="w-full bg-green-600 hover:bg-green-700 text-white"
              :disabled="!effectiveExecutable || !installPath || running"
            >
              <Play v-if="!running" class="w-4 h-4 mr-2" />
              <Loader2 v-else class="w-4 h-4 mr-2 animate-spin" />
              {{ running ? t('game_sim.starting') : t('game_sim.run_game') }}
            </Button>

            <Button
              @click="openCreateDialog"
              variant="outline"
              class="w-full"
              :disabled="!effectiveExecutable"
            >
              <Hammer class="w-4 h-4 mr-2" />
              {{ t('game_sim.create_game') }}
            </Button>
          </div>
        </CardFooter>
      </Card>
    </div>

    <!-- Create Simulated Game Dialog -->
    <Dialog v-model:open="showCreateDialog">
      <DialogContent class="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{{ t('game_sim.create_dialog_title') }}</DialogTitle>
          <DialogDescription>{{ t('game_sim.create_dialog_desc') }}</DialogDescription>
        </DialogHeader>

        <div class="space-y-4 py-2">
          <div class="space-y-2">
            <Label class="flex items-center gap-1.5">
              <FolderOpen class="w-3.5 h-3.5" />
              {{ t('game_sim.create_dialog_path_label') }}
            </Label>
            <div class="flex gap-2">
              <Input v-model="dialogSavePath" :placeholder="installPath" class="flex-1" />
              <Button type="button" variant="outline" size="icon" @click="pickDialogFolder" class="shrink-0">
                <FolderOpen class="w-4 h-4" />
              </Button>
            </div>
            <p class="text-xs text-muted-foreground">{{ t('game_sim.create_dialog_path_hint') }}</p>
          </div>

          <div v-if="error" class="p-3 bg-destructive/10 text-destructive rounded-md text-sm">{{ error }}</div>
        </div>

        <DialogFooter>
          <Button variant="outline" @click="showCreateDialog = false">
            {{ t('dialog.cancel') }}
          </Button>
          <Button
            @click="handleCreateGame"
            :disabled="!dialogSavePath || creating"
          >
            <Hammer v-if="!creating" class="w-4 h-4 mr-2" />
            <Loader2 v-else class="w-4 h-4 mr-2 animate-spin" />
            {{ creating ? t('game_sim.creating') : t('game_sim.create_game') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>

