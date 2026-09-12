<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { AlertTriangle, Bot, Check, Copy, FolderOpen, Gamepad2, MonitorPlay, RotateCw, Wifi } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { open as openFolderPicker } from '@tauri-apps/plugin-dialog'
import { mkdir } from '@tauri-apps/plugin-fs'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useQuestsStore } from '@/stores/quests'
import { navigateToTab } from '@/utils/navigate'
import AdvancedDisclosure from './AdvancedDisclosure.vue'
import SettingRow from './SettingRow.vue'
import SettingsSectionCard from './SettingsSectionCard.vue'
import SettingsStatusPanel from './SettingsStatusPanel.vue'
import { cn } from '@/lib/utils'
import { settingToneClass } from './settingTones'

const { t } = useI18n()
const questsStore = useQuestsStore()
const copied = ref(false)
const simulationDirectoryError = ref<string | null>(null)

async function copyPath() {
  simulationDirectoryError.value = null
  try {
    const path = await questsStore.initSimulationPath()
    await navigator.clipboard.writeText(path)
    copied.value = true
    setTimeout(() => { copied.value = false }, 2000)
  } catch {
    simulationDirectoryError.value = t('settings.simulation_directory_copy_error')
  }
}

async function selectSimulationDirectory() {
  simulationDirectoryError.value = null
  try {
    const currentPath = await questsStore.initSimulationPath()
    const selected = await openFolderPicker({
      directory: true,
      multiple: false,
      defaultPath: currentPath,
      title: t('settings.select_simulation_directory'),
    })
    if (typeof selected === 'string') {
      questsStore.setSimulationPath(selected)
    }
  } catch {
    simulationDirectoryError.value = t('settings.simulation_directory_select_error')
  }
}

async function resetSimulationDirectory() {
  simulationDirectoryError.value = null
  try {
    const path = await questsStore.resetSimulationPath()
    await mkdir(path, { recursive: true })
  } catch {
    simulationDirectoryError.value = t('settings.simulation_directory_reset_error')
  }
}

async function openSimulationDirectory() {
  simulationDirectoryError.value = null
  try {
    const path = await questsStore.initSimulationPath()

    // The static capability can create the default directory. Persisted custom
    // directories may not retain the picker scope after restart, but they can
    // still be opened when they already exist.
    try {
      await mkdir(path, { recursive: true })
    } catch {
      // Let the Rust command make the final existing-directory check.
    }

    await invoke('open_in_explorer', { path })
  } catch {
    simulationDirectoryError.value = t('settings.simulation_directory_open_error')
  }
}

// Validation is deferred until the input loses focus (@blur). The `Input`
// component uses `useVModel` in passive mode, which means it does NOT emit
// `update:modelValue` while typing — so a two-way binding would never capture
// the user's keystrokes. Instead we bind the display value one-way to the
// store and, on blur, read the raw value straight from the DOM and commit it
// to the store. The store's watcher then normalizes/persists it, and the
// one-way binding reflects the corrected value back.
function commitCheckpointMin(event: FocusEvent) {
  const raw = (event.target as HTMLInputElement).value
  questsStore.activityCheckpointMin = raw === '' ? Number.NaN : Number(raw)
}

function commitCheckpointMax(event: FocusEvent) {
  const raw = (event.target as HTMLInputElement).value
  questsStore.activityCheckpointMax = raw === '' ? Number.NaN : Number(raw)
}

onMounted(() => {
  questsStore.initSimulationPath().catch(() => {
    simulationDirectoryError.value = t('settings.simulation_directory_resolve_error')
  })
})
</script>

<template>
  <SettingsSectionCard
    :title="t('settings.quest_behavior_title')"
    :description="t('settings.quest_behavior_desc')"
    :icon="Gamepad2"
    tone="violet"
    content-class="space-y-6"
  >
      <div class="space-y-3">
        <Label>{{ t('settings.game_quest_mode') }}</Label>
        <div class="grid gap-3 md:grid-cols-2">
          <button
            @click="questsStore.gameQuestMode = 'simulate'"
            :class="cn(
              'rounded-lg border-2 p-4 text-left transition-all hover:-translate-y-0.5 hover:shadow-sm',
              questsStore.gameQuestMode === 'simulate'
                ? 'border-primary bg-primary/10 text-primary'
                : 'border-border bg-card hover:border-primary/40 hover:bg-primary/5',
            )"
          >
            <div class="flex items-start justify-between gap-3">
              <div class="flex min-w-0 gap-3">
                <div :class="cn('flex h-10 w-10 shrink-0 items-center justify-center rounded-md', settingToneClass.primary.icon)">
                  <Bot class="h-5 w-5" />
                </div>
                <div class="min-w-0">
                  <div class="font-semibold">{{ t('settings.game_mode_simulate') }}</div>
                  <div class="mt-1 text-xs text-muted-foreground">{{ t('settings.game_mode_simulate_desc') }}</div>
                </div>
              </div>
              <Badge
                v-if="questsStore.gameQuestMode === 'simulate'"
                variant="outline"
                :class="cn('shrink-0 text-[10px]', settingToneClass.primary.badge)"
              >
                {{ t('settings.status_ok') }}
              </Badge>
            </div>
          </button>

          <button
            @click="questsStore.cdpAvailable ? questsStore.gameQuestMode = 'cdp' : navigateToTab('settings', 'discord_integration')"
            :class="cn(
              'rounded-lg border-2 p-4 text-left transition-all hover:-translate-y-0.5 hover:shadow-sm',
              questsStore.gameQuestMode === 'cdp'
                ? 'border-emerald-500 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300'
                : questsStore.cdpAvailable
                  ? 'border-border bg-card hover:border-emerald-500/40 hover:bg-emerald-500/5'
                  : 'border-border bg-card opacity-80 hover:border-amber-500/40 hover:bg-amber-500/5',
            )"
          >
            <div class="flex items-start justify-between gap-3">
              <div class="flex min-w-0 gap-3">
                <div :class="cn('flex h-10 w-10 shrink-0 items-center justify-center rounded-md', settingToneClass[questsStore.cdpAvailable ? 'success' : 'warning'].icon)">
                  <MonitorPlay v-if="questsStore.cdpAvailable" class="h-5 w-5" />
                  <Wifi v-else class="h-5 w-5" />
                </div>
                <div class="min-w-0">
                  <div class="font-semibold">{{ t('settings.game_mode_cdp') }}</div>
                  <div class="mt-1 text-xs text-muted-foreground">
                    <template v-if="questsStore.cdpAvailable">{{ t('settings.game_mode_cdp_desc') }}</template>
                    <template v-else>{{ t('settings.game_mode_cdp_unavailable') }}</template>
                  </div>
                </div>
              </div>
              <Badge
                variant="outline"
                :class="cn('shrink-0 text-[10px]', settingToneClass[questsStore.cdpAvailable ? 'success' : 'warning'].badge)"
              >
                {{ questsStore.cdpAvailable ? t('settings.game_mode_cdp_connected') : t('settings.status_attention') }}
              </Badge>
            </div>
          </button>
        </div>
      </div>

      <SettingsStatusPanel v-if="questsStore.gameQuestMode === 'cdp'" tone="info" :icon="AlertTriangle">
        {{ t('settings.video_config_cdp_notice') }}
      </SettingsStatusPanel>

      <div class="space-y-3">
        <Label>{{ t('settings.video_task_settings') }}</Label>
        <div class="rounded-lg border px-4">
          <SettingRow
            :label="t('settings.completion_speed')"
            :description="t('settings.speed_hint')"
          >
            <div class="flex items-center gap-3">
              <input
                type="range"
                v-model.number="questsStore.speedMultiplier"
                min="0.1"
                max="2.0"
                step="0.1"
                :disabled="questsStore.gameQuestMode === 'cdp'"
                :aria-label="t('settings.completion_speed')"
                class="w-48 accent-primary disabled:opacity-50"
              />
              <Badge variant="outline" :class="settingToneClass.primary.badge">
                {{ questsStore.speedMultiplier }}x
              </Badge>
            </div>
          </SettingRow>

          <SettingRow
            :label="t('settings.request_interval')"
            :description="t('settings.interval_hint')"
          >
            <div class="flex items-center gap-3">
              <input
                type="range"
                v-model.number="questsStore.heartbeatInterval"
                min="10"
                max="30"
                step="1"
                :disabled="questsStore.gameQuestMode === 'cdp'"
                :aria-label="t('settings.request_interval')"
                class="w-48 accent-primary disabled:opacity-50"
              />
              <Badge variant="outline" :class="settingToneClass.primary.badge">
                {{ questsStore.heartbeatInterval }}s
              </Badge>
            </div>
          </SettingRow>
        </div>
      </div>

      <div class="space-y-3">
        <Label>{{ t('settings.general_task_settings') }}</Label>
        <div class="rounded-lg border px-4">
          <SettingRow
            :label="t('settings.game_polling_interval')"
            :description="t('settings.game_polling_hint')"
          >
            <div class="flex items-center gap-3">
              <input
                type="range"
                v-model.number="questsStore.gamePollingInterval"
                min="30"
                max="300"
                step="1"
                :aria-label="t('settings.game_polling_interval')"
                class="w-48 accent-primary"
              />
              <Badge variant="outline" :class="settingToneClass.violet.badge">
                {{ questsStore.gamePollingInterval }}s
              </Badge>
            </div>
          </SettingRow>
        </div>
      </div>

      <div class="space-y-3 rounded-lg border border-sky-500/25 bg-sky-500/5 p-4">
        <div>
          <Label>{{ t('settings.simulation_directory') }}</Label>
          <p class="mt-1 text-xs text-muted-foreground">{{ t('settings.simulation_directory_desc') }}</p>
        </div>
        <div class="flex items-center gap-2 rounded-md border bg-background/80 p-3" v-if="questsStore.simulationPath">
          <code class="flex-1 break-all text-xs font-mono">{{ questsStore.simulationPath }}</code>
          <Button
            variant="ghost"
            size="icon"
            :aria-label="t('debug.copy')"
            class="h-7 w-7 shrink-0 text-sky-700 hover:bg-sky-500/10 hover:text-sky-700 dark:text-sky-300 dark:hover:text-sky-300"
            @click="copyPath"
          >
            <Check v-if="copied" class="h-3.5 w-3.5" />
            <Copy v-else class="h-3.5 w-3.5" />
          </Button>
        </div>
        <p v-if="simulationDirectoryError" class="text-sm text-destructive">{{ simulationDirectoryError }}</p>
        <div class="flex flex-wrap gap-2">
          <Button variant="outline" :class="cn('gap-2', settingToneClass.info.buttonSoft)" @click="selectSimulationDirectory">
            <FolderOpen class="h-4 w-4" />
            {{ t('settings.select_simulation_directory') }}
          </Button>
          <Button variant="outline" class="gap-2" @click="resetSimulationDirectory">
            <RotateCw class="h-4 w-4" />
            {{ t('settings.reset_simulation_directory') }}
          </Button>
          <Button variant="outline" class="gap-2" @click="openSimulationDirectory">
            <FolderOpen class="h-4 w-4" />
            {{ t('settings.open_simulation_directory') }}
          </Button>
        </div>
      </div>

      <AdvancedDisclosure
        :title="t('settings.activity_timing_advanced')"
        :description="t('settings.activity_timing_advanced_desc')"
        tone="warning"
        default-open
      >
        <SettingsStatusPanel v-if="!questsStore.cdpAvailable" tone="warning" :icon="AlertTriangle" class="mb-4">
          {{ t('settings.activity_cdp_required') }}
        </SettingsStatusPanel>
        <div class="grid gap-4 md:grid-cols-2">
          <div class="space-y-2">
            <Label>{{ t('settings.activity_checkpoint_min') }}</Label>
            <div class="flex items-center gap-2">
              <Input
                type="number"
                :model-value="questsStore.activityCheckpointMin"
                min="30"
                :max="questsStore.activityCheckpointMax"
                :aria-label="t('settings.activity_checkpoint_min')"
                class="w-24"
                @blur="commitCheckpointMin"
              />
              <span class="text-sm text-muted-foreground">{{ t('settings.activity_checkpoint_unit') }}</span>
            </div>
          </div>

          <div class="space-y-2">
            <Label>{{ t('settings.activity_checkpoint_max') }}</Label>
            <div class="flex items-center gap-2">
              <Input
                type="number"
                :model-value="questsStore.activityCheckpointMax"
                :min="questsStore.activityCheckpointMin"
                max="900"
                :aria-label="t('settings.activity_checkpoint_max')"
                class="w-24"
                @blur="commitCheckpointMax"
              />
              <span class="text-sm text-muted-foreground">{{ t('settings.activity_checkpoint_unit') }}</span>
            </div>
          </div>
        </div>
      </AdvancedDisclosure>
  </SettingsSectionCard>
</template>
