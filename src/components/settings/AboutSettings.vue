<script setup lang="ts">
import { ref } from 'vue'
import { AlertTriangle, CheckCircle2, ExternalLink, Info, Link2, XCircle } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { open } from '@tauri-apps/plugin-shell'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle
} from '@/components/ui/alert-dialog'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { useVersionStore } from '@/stores/version'
import SettingsSectionCard from './SettingsSectionCard.vue'
import SettingsStatusPanel from './SettingsStatusPanel.vue'
import SettingsSwitch from './SettingsSwitch.vue'
import { cn } from '@/lib/utils'
import { settingToneClass } from './settingTones'

const { t } = useI18n()
const versionStore = useVersionStore()

const emit = defineEmits<{
  debugUnlocked: []
  debugDisabled: []
}>()

const debugModeEnabled = ref(localStorage.getItem('debugMode') === 'true')
const disableDialogOpen = ref(false)
const versionTapCount = ref(0)
const lastTapTime = ref(0)
const showDebugUnlockHint = ref(false)

interface LogoBubble {
  id: number
  style: Record<string, string>
}

const logoBubbles = ref<LogoBubble[]>([])
let bubbleId = 0

async function openExternal(url: string) {
  try {
    await open(url)
  } catch (error) {
    console.error('Failed to open URL:', error)
  }
}

function handleVersionTap() {
  if (debugModeEnabled.value) return

  const now = Date.now()
  if (now - lastTapTime.value > 2000) {
    versionTapCount.value = 0
    showDebugUnlockHint.value = false
  }
  lastTapTime.value = now
  versionTapCount.value++

  if (versionTapCount.value >= 4 && versionTapCount.value < 7) {
    showDebugUnlockHint.value = true
  }

  if (versionTapCount.value >= 7) {
    debugModeEnabled.value = true
    localStorage.setItem('debugMode', 'true')
    versionTapCount.value = 0
    showDebugUnlockHint.value = false
    emit('debugUnlocked')
  }
}

function handleVersionTapWithBubble() {
  const count = 1 + Math.floor(Math.random() * 2)
  for (let i = 0; i < count; i++) {
    bubbleId++
    const drift = (Math.random() - 0.5) * 60
    const rise = 70 + Math.random() * 50
    const scale = 0.7 + Math.random() * 0.6
    const duration = 1100 + Math.random() * 500
    logoBubbles.value.push({
      id: bubbleId,
      style: {
        '--bubble-drift': `${drift}px`,
        '--bubble-rise': `-${rise}px`,
        '--bubble-scale': `${scale}`,
        animationDuration: `${duration}ms`,
        animationDelay: `${i * 80}ms`,
      },
    })
  }
  handleVersionTap()
}

function confirmDisableDebugMode() {
  debugModeEnabled.value = false
  localStorage.removeItem('debugMode')
  emit('debugDisabled')
}

function removeBubble(id: number) {
  const idx = logoBubbles.value.findIndex(bubble => bubble.id === id)
  if (idx !== -1) logoBubbles.value.splice(idx, 1)
}
</script>

<template>
  <div class="grid gap-6 xl:grid-cols-2">
    <AlertDialog :open="disableDialogOpen" @update:open="disableDialogOpen = $event">
      <AlertDialogContent class="max-w-[480px]">
        <AlertDialogHeader>
          <AlertDialogTitle>{{ t('settings.disable_debug_confirm_title') }}</AlertDialogTitle>
          <AlertDialogDescription>{{ t('settings.disable_debug_confirm_desc') }}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{{ t('dialog.cancel') }}</AlertDialogCancel>
          <AlertDialogAction @click="confirmDisableDebugMode">
            {{ t('settings.disable_developer_mode') }}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>

    <SettingsSectionCard
      :title="t('settings.about')"
      :icon="Info"
      tone="primary"
      content-class="space-y-4 text-sm text-muted-foreground"
    >
        <div class="flex flex-wrap items-center gap-2 rounded-lg border bg-muted/30 px-3 py-2">
          <span
            class="relative cursor-pointer select-none transition-transform active:scale-95"
            @click="handleVersionTapWithBubble"
            title="Version Info"
          >
            Discord Quest Helper v{{ versionStore.currentVersion }}
            <img
              v-for="bubble in logoBubbles"
              :key="bubble.id"
              src="/icons/logo.png"
              alt=""
              class="logo-bubble pointer-events-none absolute bottom-0 left-1/2 z-50 -ml-4 h-8 w-8"
              :style="bubble.style"
              @animationend="removeBubble(bubble.id)"
            />
          </span>
          <Badge v-if="versionStore.isLatest" variant="outline" :class="cn('gap-1', settingToneClass.success.badge)">
            <CheckCircle2 class="h-3 w-3" />
            {{ t('settings.version_latest') }}
          </Badge>
          <span v-else-if="versionStore.isChecking" class="text-xs text-muted-foreground">
            {{ t('settings.version_checking') }}
          </span>
          <Badge v-if="debugModeEnabled" variant="outline" :class="cn('gap-1', settingToneClass.success.badge)">
            <CheckCircle2 class="h-3 w-3" />
            {{ t('settings.debug_already_unlocked') }}
          </Badge>
          <Button
            v-if="debugModeEnabled"
            variant="outline"
            size="sm"
            :class="cn('h-7 gap-1 px-2 text-xs', settingToneClass.danger.buttonSoft)"
            @click="disableDialogOpen = true"
          >
            <XCircle class="h-3 w-3" />
            {{ t('settings.disable_developer_mode') }}
          </Button>
          <span v-else-if="showDebugUnlockHint" class="animate-pulse text-xs font-medium text-primary">
            {{ t('settings.debug_unlock_hint', { steps: 7 - versionTapCount }) }}
          </span>
        </div>

        <p>{{ t('settings.about_desc') }}</p>

        <a
          href="#"
          @click.prevent="openExternal('https://github.com/Masterain98/discord-quest-helper')"
          class="flex items-center justify-between rounded-lg border bg-muted/30 px-3 py-2 transition-colors hover:bg-muted/60"
        >
          <span class="flex min-w-0 items-center gap-2">
            <img src="/icons/github-mark.svg" alt="GitHub" class="h-5 w-5 shrink-0 dark:hidden" />
            <img src="/icons/github-mark-white.svg" alt="GitHub" class="hidden h-5 w-5 shrink-0 dark:block" />
            <span class="truncate text-primary">Masterain98/discord-quest-helper</span>
          </span>
          <ExternalLink class="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
        </a>

        <div class="flex flex-wrap gap-2">
          <Button
            variant="outline"
            size="sm"
            :class="settingToneClass.info.buttonSoft"
            @click="openExternal('https://github.com/Masterain98/discord-quest-helper/issues/new/choose')"
          >
            {{ t('settings.feedback') }}
          </Button>
          <Button
            variant="outline"
            size="sm"
            :class="settingToneClass.primary.buttonSoft"
            @click="openExternal('https://discord-quest-helper.dal.ao/')"
          >
            {{ t('settings.website') }}
          </Button>
        </div>

        <div class="rounded-lg border px-4 py-3">
          <div class="flex items-center justify-between gap-3">
            <div class="space-y-0.5">
              <Label class="text-sm font-medium">{{ t('settings.check_prerelease') }}</Label>
              <p class="text-xs text-muted-foreground">{{ t('settings.check_prerelease_desc') }}</p>
            </div>
            <SettingsSwitch
              :model-value="versionStore.checkPreRelease"
              @update:model-value="versionStore.setCheckPreRelease"
            />
          </div>
        </div>

        <SettingsStatusPanel tone="warning" :icon="AlertTriangle">
          {{ t('settings.about_warning') }}
        </SettingsStatusPanel>
    </SettingsSectionCard>

    <SettingsSectionCard
      :title="t('settings.credits')"
      :icon="Link2"
      tone="info"
      content-class="space-y-4 text-sm text-muted-foreground"
    >
        <div>
          <p class="mb-2 font-medium text-foreground">{{ t('settings.credits_desc') }}</p>
          <ul class="space-y-2">
            <li>
              <a href="#" @click.prevent="openExternal('https://github.com/markterence/discord-quest-completer')" class="flex items-center justify-between rounded-lg border bg-card px-3 py-2 text-sm transition-colors hover:bg-muted/50">
                <span class="flex min-w-0 items-center gap-2">
                  <img src="/icons/github-mark.svg" alt="GitHub" class="h-4 w-4 shrink-0 dark:hidden" />
                  <img src="/icons/github-mark-white.svg" alt="GitHub" class="hidden h-4 w-4 shrink-0 dark:block" />
                  <span class="truncate">markterence/discord-quest-completer</span>
                </span>
                <ExternalLink class="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              </a>
            </li>
            <li>
              <a href="#" @click.prevent="openExternal('https://github.com/power0matin/discord-quest-auto-completer')" class="flex items-center justify-between rounded-lg border bg-card px-3 py-2 text-sm transition-colors hover:bg-muted/50">
                <span class="flex min-w-0 items-center gap-2">
                  <img src="/icons/github-mark.svg" alt="GitHub" class="h-4 w-4 shrink-0 dark:hidden" />
                  <img src="/icons/github-mark-white.svg" alt="GitHub" class="hidden h-4 w-4 shrink-0 dark:block" />
                  <span class="truncate">power0matin/discord-quest-auto-completer</span>
                </span>
                <ExternalLink class="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              </a>
            </li>
            <li>
              <a href="#" @click.prevent="openExternal('https://github.com/taisrisk/Discord-Quest-Helper')" class="flex items-center justify-between rounded-lg border bg-card px-3 py-2 text-sm transition-colors hover:bg-muted/50">
                <span class="flex min-w-0 items-center gap-2">
                  <img src="/icons/github-mark.svg" alt="GitHub" class="h-4 w-4 shrink-0 dark:hidden" />
                  <img src="/icons/github-mark-white.svg" alt="GitHub" class="hidden h-4 w-4 shrink-0 dark:block" />
                  <span class="truncate">taisrisk/Discord-Quest-Helper</span>
                </span>
                <ExternalLink class="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              </a>
            </li>
            <li>
              <a href="#" @click.prevent="openExternal('https://gist.github.com/aamiaa/204cd9d42013ded9faf646fae7f89fbb')" class="flex items-center justify-between rounded-lg border bg-card px-3 py-2 text-sm transition-colors hover:bg-muted/50">
                <span class="flex min-w-0 items-center gap-2">
                  <img src="/icons/github-mark.svg" alt="GitHub" class="h-4 w-4 shrink-0 dark:hidden" />
                  <img src="/icons/github-mark-white.svg" alt="GitHub" class="hidden h-4 w-4 shrink-0 dark:block" />
                  <span class="truncate">aamiaa/CompleteDiscordQuest.md</span>
                </span>
                <ExternalLink class="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              </a>
            </li>
            <li>
              <a href="#" @click.prevent="openExternal('https://docs.discord.food/')" class="flex items-center justify-between rounded-lg border bg-card px-3 py-2 text-sm transition-colors hover:bg-muted/50">
                <span class="flex min-w-0 items-center gap-2">
                  <Link2 class="h-4 w-4 shrink-0" />
                  <span class="truncate">docs.discord.food</span>
                </span>
                <ExternalLink class="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              </a>
            </li>
          </ul>
        </div>
        <div>
          <p class="mb-1 font-medium text-foreground">{{ t('settings.tech_stack') }}</p>
          <div class="flex flex-wrap gap-2">
            <Badge variant="outline">Tauri</Badge>
            <Badge variant="outline">Vue 3</Badge>
            <Badge variant="outline">shadcn-vue</Badge>
            <Badge variant="outline">TailwindCSS</Badge>
            <Badge variant="outline">vue-i18n</Badge>
          </div>
        </div>
    </SettingsSectionCard>
  </div>
</template>

<style scoped>
@keyframes logoBubbleRise {
  0% {
    opacity: 0;
    transform: translate(-50%, 0) scale(0.5);
  }
  15% {
    opacity: 1;
  }
  100% {
    opacity: 0;
    transform: translate(calc(-50% + var(--bubble-drift)), var(--bubble-rise)) scale(var(--bubble-scale));
  }
}

.logo-bubble {
  animation-name: logoBubbleRise;
  animation-timing-function: cubic-bezier(0.25, 0.46, 0.45, 0.94);
  animation-fill-mode: forwards;
  filter: drop-shadow(0 2px 6px rgba(0, 0, 0, 0.15));
}
</style>
