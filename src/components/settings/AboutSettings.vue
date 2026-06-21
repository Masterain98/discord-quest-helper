<script setup lang="ts">
import { ref } from 'vue'
import { CheckCircle2, Link2 } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { open } from '@tauri-apps/plugin-shell'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Label } from '@/components/ui/label'
import { useVersionStore } from '@/stores/version'

const { t } = useI18n()
const versionStore = useVersionStore()

const emit = defineEmits<{
  debugUnlocked: []
}>()

const debugModeEnabled = ref(localStorage.getItem('debugMode') === 'true')
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

function removeBubble(id: number) {
  const idx = logoBubbles.value.findIndex(bubble => bubble.id === id)
  if (idx !== -1) logoBubbles.value.splice(idx, 1)
}
</script>

<template>
  <div class="grid gap-6 xl:grid-cols-2">
    <Card>
      <CardHeader>
        <CardTitle>{{ t('settings.about') }}</CardTitle>
      </CardHeader>
      <CardContent class="space-y-4 text-sm text-muted-foreground">
        <p class="flex flex-wrap items-center gap-2">
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
          <Badge v-if="versionStore.isLatest" variant="outline" class="gap-1 border-green-600/50 text-green-600">
            <CheckCircle2 class="h-3 w-3" />
            {{ t('settings.version_latest') }}
          </Badge>
          <span v-else-if="versionStore.isChecking" class="text-xs text-muted-foreground">
            {{ t('settings.version_checking') }}
          </span>
          <span v-if="debugModeEnabled" class="inline-flex items-center gap-1 text-xs font-medium text-green-600 dark:text-green-400">
            <CheckCircle2 class="h-3 w-3" />
            {{ t('settings.debug_already_unlocked') }}
          </span>
          <span v-else-if="showDebugUnlockHint" class="animate-pulse text-xs font-medium text-primary">
            {{ t('settings.debug_unlock_hint', { steps: 7 - versionTapCount }) }}
          </span>
        </p>

        <p>{{ t('settings.about_desc') }}</p>

        <a href="#" @click.prevent="openExternal('https://github.com/Masterain98/discord-quest-helper')" class="inline-flex items-center gap-2 transition-opacity hover:opacity-80">
          <img src="/icons/github-mark.svg" alt="GitHub" class="h-5 w-5 dark:hidden" />
          <img src="/icons/github-mark-white.svg" alt="GitHub" class="hidden h-5 w-5 dark:block" />
          <span class="text-primary hover:underline">Masterain98/discord-quest-helper</span>
        </a>

        <div class="flex flex-wrap gap-2">
          <Button variant="outline" size="sm" @click="openExternal('https://github.com/Masterain98/discord-quest-helper/issues/new/choose')">
            {{ t('settings.feedback') }}
          </Button>
          <Button variant="outline" size="sm" @click="openExternal('https://discord-quest-helper.dal.ao/')">
            {{ t('settings.website') }}
          </Button>
        </div>

        <div class="rounded-lg border px-4 py-3">
          <div class="flex items-center justify-between gap-3">
            <div class="space-y-0.5">
              <Label class="text-sm font-medium">{{ t('settings.check_prerelease') }}</Label>
              <p class="text-xs text-muted-foreground">{{ t('settings.check_prerelease_desc') }}</p>
            </div>
            <button
              type="button"
              role="switch"
              :aria-checked="versionStore.checkPreRelease"
              :class="[
                'peer inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full border-2 border-transparent transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:cursor-not-allowed disabled:opacity-50',
                versionStore.checkPreRelease ? 'bg-primary' : 'bg-input',
              ]"
              @click="versionStore.setCheckPreRelease(!versionStore.checkPreRelease)"
            >
              <span
                :class="[
                  'pointer-events-none block h-4 w-4 rounded-full bg-background shadow-lg ring-0 transition-transform',
                  versionStore.checkPreRelease ? 'translate-x-4' : 'translate-x-0',
                ]"
              />
            </button>
          </div>
        </div>

        <p class="text-yellow-500/90 dark:text-yellow-400">
          ⚠️ {{ t('settings.about_warning') }}
        </p>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>{{ t('settings.credits') }}</CardTitle>
      </CardHeader>
      <CardContent class="space-y-4 text-sm text-muted-foreground">
        <div>
          <p class="mb-2 font-medium text-foreground">{{ t('settings.credits_desc') }}</p>
          <ul class="space-y-2">
            <li>
              <a href="#" @click.prevent="openExternal('https://github.com/markterence/discord-quest-completer')" class="inline-flex items-center gap-2 transition-opacity hover:opacity-80">
                <img src="/icons/github-mark.svg" alt="GitHub" class="h-4 w-4 dark:hidden" />
                <img src="/icons/github-mark-white.svg" alt="GitHub" class="hidden h-4 w-4 dark:block" />
                <span class="hover:underline">markterence/discord-quest-completer</span>
              </a>
            </li>
            <li>
              <a href="#" @click.prevent="openExternal('https://github.com/power0matin/discord-quest-auto-completer')" class="inline-flex items-center gap-2 transition-opacity hover:opacity-80">
                <img src="/icons/github-mark.svg" alt="GitHub" class="h-4 w-4 dark:hidden" />
                <img src="/icons/github-mark-white.svg" alt="GitHub" class="hidden h-4 w-4 dark:block" />
                <span class="hover:underline">power0matin/discord-quest-auto-completer</span>
              </a>
            </li>
            <li>
              <a href="#" @click.prevent="openExternal('https://github.com/taisrisk/Discord-Quest-Helper')" class="inline-flex items-center gap-2 transition-opacity hover:opacity-80">
                <img src="/icons/github-mark.svg" alt="GitHub" class="h-4 w-4 dark:hidden" />
                <img src="/icons/github-mark-white.svg" alt="GitHub" class="hidden h-4 w-4 dark:block" />
                <span class="hover:underline">taisrisk/Discord-Quest-Helper</span>
              </a>
            </li>
            <li>
              <a href="#" @click.prevent="openExternal('https://gist.github.com/aamiaa/204cd9d42013ded9faf646fae7f89fbb')" class="inline-flex items-center gap-2 transition-opacity hover:opacity-80">
                <img src="/icons/github-mark.svg" alt="GitHub" class="h-4 w-4 dark:hidden" />
                <img src="/icons/github-mark-white.svg" alt="GitHub" class="hidden h-4 w-4 dark:block" />
                <span class="hover:underline">aamiaa/CompleteDiscordQuest.md</span>
              </a>
            </li>
            <li>
              <a href="#" @click.prevent="openExternal('https://docs.discord.food/')" class="inline-flex items-center gap-2 transition-opacity hover:opacity-80">
                <Link2 class="h-4 w-4" />
                <span class="hover:underline">docs.discord.food</span>
              </a>
            </li>
          </ul>
        </div>
        <div>
          <p class="mb-1 font-medium text-foreground">{{ t('settings.tech_stack') }}</p>
          <ul class="list-inside list-disc">
            <li>Tauri</li>
            <li>Vue 3</li>
            <li>shadcn-vue</li>
            <li>TailwindCSS</li>
            <li>vue-i18n</li>
          </ul>
        </div>
      </CardContent>
    </Card>
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
