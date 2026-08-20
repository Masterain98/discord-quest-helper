<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import GameSelector from '@/components/GameSelector.vue'
import type { DetectableGame, ManualCdpGameSimulation } from '@/api/tauri'
import {
  createSimulatedGame,
  runSimulatedGame,
  stopSimulatedGame,
  connectToDiscordRpc,
  disconnectFromDiscordRpc,
  startManualCdpGameSimulation,
  stopManualCdpGameSimulation,
  getManualCdpGameSimulation,
} from '@/api/tauri'
import { documentDir, sep } from '@tauri-apps/api/path'
import { open as openFolderPicker } from '@tauri-apps/plugin-dialog'
import { Card, CardHeader, CardTitle, CardContent, CardDescription, CardFooter } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from '@/components/ui/dialog'
import { Loader2, Play, Square, Hammer, List, Terminal, FolderOpen, ChevronDown, Check, MonitorPlay, WifiOff } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { useQuestsStore } from '@/stores/quests'
import { getSimulationExecutables } from '@/utils/executables'

const { t } = useI18n()
const store = useQuestsStore()

// Avoid offering a platform-specific executable until the backend descriptor is
// known; otherwise a failed capability load could silently select win32 on Linux.
const executablePriority = computed(() => store.platformCapabilities?.executableOsPriority ?? [])
const hostOs = computed(() => store.platformCapabilities?.os ?? '')

// Mode: 'select' = pick from detectable games list, 'custom' = enter any process name
const mode = ref<'select' | 'custom'>('select')

const selectedGame = ref<DetectableGame | null>(null)
const selectedExecutable = ref('')
const customExeName = ref('')
const installPath = ref('')
const installPathPlaceholder = ref('DiscordQuestGames')
const running = ref(false)
const stopping = ref(false)
const activeExecutable = ref<string | null>(null)
const activeRpc = ref(false)
const activeSimulationMode = ref<'process' | 'cdp' | null>(null)
const activeCdpSession = ref<ManualCdpGameSimulation | null>(null)
const cdpStarting = ref(false)
const creating = ref(false)
const error = ref<string | null>(null)
const success = ref<string | null>(null)

// Create dialog state
const showCreateDialog = ref(false)
const dialogSavePath = ref('')

onMounted(async () => {
  const capabilities = store.initPlatformCapabilities()
  const cdpStatus = store.initCdpMode().catch(err => {
    console.warn('Failed to refresh CDP status for game simulator:', err)
  })
  const manualSession = getManualCdpGameSimulation().catch(err => {
    console.warn('Failed to restore manual CDP game simulation:', err)
    return null
  })
  const [docDir, separator, session] = await Promise.all([documentDir(), sep(), manualSession])
  installPathPlaceholder.value = `${docDir}${separator}DiscordQuestGames`
  installPath.value = installPathPlaceholder.value
  await Promise.all([capabilities, cdpStatus])

  if (session) {
    activeSimulationMode.value = 'cdp'
    activeCdpSession.value = session
    success.value = t('game_sim.cdp_session_restored', { name: session.appName })
  }
})

const hasActiveSimulation = computed(() => activeSimulationMode.value !== null)
const simulatorBusy = computed(
  () => running.value || creating.value || cdpStarting.value || stopping.value
)

// Executables the simulator can actually launch here: Linux only runs a native
// `linux` binary (a win32 exe is refused by the quest-start path too), while
// Windows/macOS stay win32-only.
const compatibleExecutables = computed(() => {
  if (!selectedGame.value || !store.platformCapabilities) return []
  return getSimulationExecutables(selectedGame.value.executables, hostOs.value, executablePriority.value)
})

const hasCompatibleExecutables = computed(() => compatibleExecutables.value.length > 0)

// On Linux a win32-only game isn't "unknown to Discord" — it just can't be
// process-simulated here, so explain that instead of the generic hint.
const isWin32OnlyOnLinux = computed(
  () =>
    hostOs.value === 'linux' &&
    !hasCompatibleExecutables.value &&
    !!selectedGame.value?.executables.some((exe) => exe.os === 'win32')
)

// The executable name that will actually be used for run/create
const effectiveExecutable = computed(() => {
  if (mode.value === 'custom') return customExeName.value
  if (hasCompatibleExecutables.value) return selectedExecutable.value
  return selectModeCustomExe.value
})

// In select mode, a custom exe name is provided when the game has no known win32 executables
const selectModeCustomExe = ref('')

// Custom exe dropdown state
const exeDropdownOpen = ref(false)
const exeDropdownRef = ref<HTMLElement | null>(null)

function toggleExeDropdown() {
  exeDropdownOpen.value = !exeDropdownOpen.value
}

function selectExe(name: string) {
  selectedExecutable.value = name
  exeDropdownOpen.value = false
}

function handleClickOutsideExeDropdown(e: MouseEvent) {
  if (exeDropdownRef.value && !exeDropdownRef.value.contains(e.target as Node)) {
    exeDropdownOpen.value = false
  }
}

onMounted(() => document.addEventListener('mousedown', handleClickOutsideExeDropdown))
onUnmounted(() => document.removeEventListener('mousedown', handleClickOutsideExeDropdown))

// Whether the footer action buttons should be shown
const canProceed = computed(() => {
  if (hasActiveSimulation.value) return true
  if (mode.value === 'custom') return !!customExeName.value
  // CDP simulation only needs the selected Discord application ID, so keep
  // the footer available even when no local executable can be simulated.
  if (!selectedGame.value) return false
  return true
})

function switchMode(m: 'select' | 'custom') {
  if (hasActiveSimulation.value || simulatorBusy.value) return
  mode.value = m
  error.value = null
  success.value = null
}

function selectGame(game: DetectableGame) {
  if (hasActiveSimulation.value || simulatorBusy.value) return
  selectedGame.value = game
  const compatible = store.platformCapabilities
    ? getSimulationExecutables(game.executables, hostOs.value, executablePriority.value)
    : []
  selectedExecutable.value = compatible[0]?.name ?? ''
  selectModeCustomExe.value = ''
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
  const exeName = effectiveExecutable.value
  if (!exeName || !dialogSavePath.value) return

  creating.value = true
  error.value = null
  success.value = null

  try {
    const appId = mode.value === 'custom' ? '' : (selectedGame.value?.id ?? '')
    await createSimulatedGame(dialogSavePath.value, exeName, appId)
    showCreateDialog.value = false
    success.value = t('game_sim.create_success')
  } catch (e) {
    error.value = e as string
  } finally {
    creating.value = false
  }
}

async function handleRunGame() {
  // Resolve which exe name to use
  const exeName = effectiveExecutable.value
  if (!exeName || !installPath.value || creating.value || hasActiveSimulation.value) return

  running.value = true
  error.value = null
  success.value = null

  try {
    const appId = mode.value === 'custom' ? '' : (selectedGame.value?.id ?? '')
    const displayName = mode.value === 'custom' ? customExeName.value : (selectedGame.value?.name ?? '')
    await runSimulatedGame(displayName, installPath.value, exeName, appId)
    activeExecutable.value = exeName
    activeSimulationMode.value = 'process'

    // ── SELECT / LIST mode ──────────────────────────────────────────────
    // When launched from the detectable games list we always have an app_id,
    // so establish a Discord RPC connection to report Rich Presence.
    // This also covers the case where the game has no known executables but
    // the user provided a custom name — we still have the app_id for RPC.
    if (mode.value === 'select' && selectedGame.value) {
      const activity = {
        app_id: selectedGame.value.id,
        large_image_key: 'logo',
        large_image_text: selectedGame.value.name,
        start_timestamp: Date.now()
      }
      await connectToDiscordRpc(JSON.stringify(activity), 'connect')
      activeRpc.value = true
      success.value = t('game_sim.run_success_rpc')
    } else {
      // ── CUSTOM mode ─────────────────────────────────────────────────
      // No app_id is available, so we cannot establish an RPC connection.
      // Detection relies entirely on Discord matching the process name
      // against its detectable-games database.
      success.value = t('game_sim.run_success')
    }
  } catch (e) {
    error.value = e as string
  } finally {
    running.value = false
  }
}

async function handleRunCdpGame() {
  const game = selectedGame.value
  if (!game || creating.value || cdpStarting.value || hasActiveSimulation.value || store.activeQuestId) return

  cdpStarting.value = true
  error.value = null
  success.value = null

  try {
    // Refresh immediately before mutation so a stale connected flag cannot
    // enable an injection after Discord has been closed or restarted.
    await store.initCdpMode()
    if (!store.cdpAvailable) {
      throw new Error(t('game_sim.cdp_unavailable'))
    }

    const session = await startManualCdpGameSimulation(game.id, game.name, store.cdpPort)
    activeCdpSession.value = session
    activeSimulationMode.value = 'cdp'
    success.value = t('game_sim.cdp_started', { name: game.name })
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    cdpStarting.value = false
  }
}

async function handleStopGame() {
  const simulationMode = activeSimulationMode.value
  if (!simulationMode || stopping.value) return

  stopping.value = true
  error.value = null
  success.value = null

  try {
    if (simulationMode === 'cdp') {
      await stopManualCdpGameSimulation()
      activeCdpSession.value = null
      activeSimulationMode.value = null
      success.value = t('game_sim.cdp_stopped')
      return
    }

    const exeName = activeExecutable.value
    if (!exeName) throw new Error(t('game_sim.no_active_process'))
    await stopSimulatedGame(exeName)
    const hadRpc = activeRpc.value
    activeExecutable.value = null
    activeSimulationMode.value = null
    activeRpc.value = false
    if (hadRpc) {
      await disconnectFromDiscordRpc()
    }
    success.value = t('game_sim.stopped')
  } catch (e) {
    const detail = e instanceof Error ? e.message : String(e)
    error.value = simulationMode === 'cdp'
      ? t('game_sim.cdp_cleanup_failed', { error: detail })
      : detail
  } finally {
    stopping.value = false
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
          :disabled="hasActiveSimulation || simulatorBusy"
          @click="switchMode('select')"
        >
          <List class="w-3.5 h-3.5" />
          {{ t('game_sim.mode_from_list') }}
        </Button>
        <Button
          size="sm"
          :variant="mode === 'custom' ? 'default' : 'ghost'"
          class="gap-1.5 h-7 px-3 text-xs"
          :disabled="hasActiveSimulation || simulatorBusy"
          @click="switchMode('custom')"
        >
          <Terminal class="w-3.5 h-3.5" />
          {{ t('game_sim.mode_custom') }}
        </Button>
      </div>
    </div>

    <div class="grid grid-cols-1 gap-6" :class="mode === 'select' ? 'lg:grid-cols-2' : ''">
      <GameSelector v-if="mode === 'select'" :disabled="hasActiveSimulation || simulatorBusy" @select="selectGame" />

      <Card>
        <CardHeader>
          <CardTitle>{{ t('game_sim.config_title') }}</CardTitle>
          <CardDescription>{{ mode === 'custom' ? t('game_sim.custom_config_desc') : t('game_sim.config_desc') }}</CardDescription>
        </CardHeader>

        <CardContent>
          <div
            v-if="activeSimulationMode === 'cdp' && activeCdpSession"
            class="mb-6 p-3 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300 rounded-md text-sm border border-emerald-500/20"
          >
            {{ t('game_sim.cdp_active', { name: activeCdpSession.appName }) }}
          </div>

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

              <div v-if="!store.platformCapabilitiesReady" class="text-center py-4 text-muted-foreground">
                {{ t('general.loading') }}
              </div>

              <div v-else-if="!store.platformCapabilities" class="p-3 bg-destructive/10 text-destructive rounded-md text-sm">
                {{ t('game_sim.platform_capabilities_unavailable') }}
              </div>

              <!-- No simulator-compatible executables — let user enter a custom name -->
              <template v-else-if="!hasCompatibleExecutables">
                <div class="p-3 bg-yellow-500/10 text-yellow-600 dark:text-yellow-400 rounded-md text-sm border border-yellow-500/20 space-y-1">
                  <p>{{ isWin32OnlyOnLinux ? t('game_sim.no_linux_exe_hint') : t('game_sim.no_exe_hint') }}</p>
                  <p>{{ t('game_sim.no_exe_custom_warning') }}</p>
                </div>

                <div class="space-y-2">
                  <Label>{{ t('game_sim.custom_exe_label') }}</Label>
                  <Input
                    v-model="selectModeCustomExe"
                    :placeholder="t('game_sim.custom_exe_placeholder')"
                  />
                </div>

                <div class="space-y-2">
                  <Label>{{ t('game_sim.install_path') }}</Label>
                  <div class="flex gap-2">
                    <Input v-model="installPath" :placeholder="installPathPlaceholder" class="flex-1" />
                    <Button type="button" variant="outline" size="icon" @click="pickInstallFolder" class="shrink-0">
                      <FolderOpen class="w-4 h-4" />
                    </Button>
                  </div>
                </div>
              </template>

              <template v-else>
                <div class="space-y-2">
                  <Label>{{ t('game_sim.select_exe') }}</Label>
                  <div ref="exeDropdownRef" class="relative">
                    <button
                      type="button"
                      class="flex h-10 w-full items-center justify-between rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background transition-colors hover:bg-accent/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
                      @click="toggleExeDropdown"
                    >
                      <span :class="selectedExecutable ? 'text-foreground' : 'text-muted-foreground'">
                        {{ selectedExecutable || t('game_sim.select_exe') }}
                      </span>
                      <ChevronDown class="w-4 h-4 text-muted-foreground shrink-0 transition-transform" :class="exeDropdownOpen && 'rotate-180'" />
                    </button>

                    <Transition
                      enter-active-class="transition ease-out duration-100"
                      enter-from-class="opacity-0 -translate-y-1"
                      enter-to-class="opacity-100 translate-y-0"
                      leave-active-class="transition ease-in duration-75"
                      leave-from-class="opacity-100 translate-y-0"
                      leave-to-class="opacity-0 -translate-y-1"
                    >
                      <div
                        v-if="exeDropdownOpen"
                        class="absolute z-50 mt-1 w-full rounded-md border bg-popover text-popover-foreground shadow-md overflow-hidden"
                      >
                        <div class="max-h-48 overflow-y-auto p-1">
                          <button
                            v-for="exe in compatibleExecutables"
                            :key="exe.name"
                            type="button"
                            class="flex w-full items-center gap-2 rounded-sm px-2.5 py-1.5 text-sm outline-none transition-colors hover:bg-accent hover:text-accent-foreground"
                            :class="selectedExecutable === exe.name && 'bg-accent/50'"
                            @click="selectExe(exe.name)"
                          >
                            <Check v-if="selectedExecutable === exe.name" class="w-4 h-4 shrink-0 text-primary" />
                            <span v-else class="w-4 shrink-0" />
                            <span class="font-mono truncate">{{ exe.name }}</span>
                          </button>
                        </div>
                      </div>
                    </Transition>
                  </div>
                </div>

                <div class="space-y-2">
                  <Label>{{ t('game_sim.install_path') }}</Label>
                  <div class="flex gap-2">
                    <Input v-model="installPath" :placeholder="installPathPlaceholder" class="flex-1" />
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
                  <Input v-model="installPath" :placeholder="installPathPlaceholder" class="flex-1" />
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
              v-if="!hasActiveSimulation"
              @click="handleRunGame"
              class="w-full bg-green-600 hover:bg-green-700 text-white"
              :disabled="!effectiveExecutable || !installPath || simulatorBusy || !!store.activeQuestId"
            >
              <Play v-if="!running" class="w-4 h-4 mr-2" />
              <Loader2 v-else class="w-4 h-4 mr-2 animate-spin" />
              {{ running ? t('game_sim.starting') : t('game_sim.run_game') }}
            </Button>

            <Button
              v-if="mode === 'select' && !hasActiveSimulation"
              @click="handleRunCdpGame"
              variant="outline"
              class="w-full border-emerald-500/50 text-emerald-700 hover:bg-emerald-500/10 dark:text-emerald-300"
              :disabled="!selectedGame || !store.cdpAvailable || simulatorBusy || !!store.activeQuestId"
              :title="store.cdpAvailable ? t('game_sim.cdp_button_hint') : t('game_sim.cdp_unavailable')"
            >
              <MonitorPlay v-if="store.cdpAvailable && !cdpStarting" class="w-4 h-4 mr-2" />
              <WifiOff v-else-if="!store.cdpAvailable && !cdpStarting" class="w-4 h-4 mr-2" />
              <Loader2 v-else class="w-4 h-4 mr-2 animate-spin" />
              {{ cdpStarting ? t('game_sim.cdp_starting') : t('game_sim.run_cdp_game') }}
            </Button>

            <Button
              v-if="hasActiveSimulation"
              @click="handleStopGame"
              variant="destructive"
              class="w-full"
              :disabled="stopping"
            >
              <Square v-if="!stopping" class="w-4 h-4 mr-2" />
              <Loader2 v-else class="w-4 h-4 mr-2 animate-spin" />
              {{ stopping ? t('game_sim.stopping') : t('game_sim.stop_game') }}
            </Button>

            <Button
              @click="openCreateDialog"
              variant="outline"
              class="w-full"
              :disabled="!effectiveExecutable || hasActiveSimulation || simulatorBusy"
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

