<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import Home from './views/Home.vue'
import GameSimulator from './views/GameSimulator.vue'
import Settings from './views/Settings.vue'
import Debug from './views/Debug.vue'
import TitleBar from './components/TitleBar.vue'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useAuthStore } from '@/stores/auth'
import { useQuestsStore } from '@/stores/quests'
import { useVersionStore } from '@/stores/version'
import {
  checkCdpStatus,
  isDiscordRunning,
  launchDiscordCdp,
  restartDiscordCdp,
  type ExtractedAccount,
} from '@/api/tauri'
import { useI18n } from 'vue-i18n'
import { Moon, Sun, Loader2, Languages } from 'lucide-vue-next'
import AccountMenu from './components/AccountMenu.vue'
import OrbsNitroStatus from './components/OrbsNitroStatus.vue'
import QuestModeIndicator from './components/QuestModeIndicator.vue'
import Toaster from './components/Toaster.vue'
import { persistSettingsSection } from '@/composables/useSettingsNavigation'
import { supportedLocales } from '@/locales/meta'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'

const { t, locale } = useI18n()
const currentTab = ref<'home' | 'game' | 'settings' | 'debug'>('home')
const authStore = useAuthStore()
const questsStore = useQuestsStore()

// Capabilities load asynchronously, so stay hidden until the descriptor is
// ready rather than flashing a button that the current platform may not use.
const showAutoDetect = computed(() => {
  if (!questsStore.platformCapabilitiesReady) return false
  const level = questsStore.platformCapabilities?.tokenAutoDetection
  return level !== 'manual_only' && level !== 'unavailable'
})

// Theme Logic
const isDark = ref(true) // Default to dark

// Debug mode state
const debugModeEnabled = ref(false)



// Account selection tracking
const selectedAccountId = ref<string | null>(null)

// Manual login
const manualTokenInput = ref('')
const cdpLoginPreparing = ref(false)
const cdpLoginRestartDialogOpen = ref(false)

async function handleManualLogin() {
  if (!manualTokenInput.value) return
  await authStore.loginWithToken(manualTokenInput.value)
  manualTokenInput.value = ''
}

async function handleAutoDetect() {
  await authStore.tryAutoDetect()
}

async function handleCdpLogin() {
  if (cdpLoginPreparing.value || authStore.loading) return
  cdpLoginPreparing.value = true
  authStore.error = null
  try {
    const status = await checkCdpStatus(questsStore.cdpPort)
    // A reachable CDP endpoint may briefly report no selected Discord target
    // while Discord is initializing. Reuse it and let the login command perform
    // the authoritative target/session check instead of restarting Discord.
    if (!status.available) {
      try {
        // This command is idempotent: if Discord CDP is already ready, the
        // launcher core returns AlreadyAvailable without restarting it.
        await launchDiscordCdp(questsStore.cdpPort, 'auto')
      } catch (launchError) {
        // Close the race where CDP became available between the first probe and
        // the launch attempt. A reachable endpoint must never trigger restart.
        const retryStatus = await checkCdpStatus(questsStore.cdpPort)
        if (!retryStatus.available) {
          const running = await isDiscordRunning('auto')
          if (running) {
            cdpLoginRestartDialogOpen.value = true
            return
          }
          throw launchError
        }
      }
    }
    await authStore.loginViaCdp()
  } catch (error) {
    authStore.error = error instanceof Error ? error.message : String(error)
  } finally {
    cdpLoginPreparing.value = false
  }
}

async function confirmCdpLoginRestart() {
  if (cdpLoginPreparing.value || authStore.loading) return
  cdpLoginRestartDialogOpen.value = false
  cdpLoginPreparing.value = true
  authStore.error = null
  try {
    await restartDiscordCdp(questsStore.cdpPort, 'auto')
    await authStore.loginViaCdp()
  } catch (error) {
    authStore.error = error instanceof Error ? error.message : String(error)
  } finally {
    cdpLoginPreparing.value = false
  }
}

function toggleTheme(event: MouseEvent) {
  // Get click coordinates for ripple origin
  const x = event.clientX
  const y = event.clientY
  
  // Calculate the end radius to cover the entire screen
  const endRadius = Math.hypot(
    Math.max(x, window.innerWidth - x),
    Math.max(y, window.innerHeight - y)
  )
  
  // Determine if switching to dark mode
  const switchingToDark = !isDark.value
  
  // Check if View Transitions API is supported
  if (document.startViewTransition) {
    // Use View Transitions API for smooth animation
    const transition = document.startViewTransition(() => {
      isDark.value = !isDark.value
      updateTheme()
    })
    
    transition.ready.then(() => {
      // For light-to-dark: shrink from full to center (reverse ripple)
      // For dark-to-light: expand from center to full
      const clipPathStart = switchingToDark 
        ? `circle(${endRadius}px at ${x}px ${y}px)`
        : `circle(0px at ${x}px ${y}px)`
      const clipPathEnd = switchingToDark 
        ? `circle(0px at ${x}px ${y}px)`
        : `circle(${endRadius}px at ${x}px ${y}px)`
      
      // Animate the old view (shrinking) when going to dark
      // Animate the new view (expanding) when going to light  
      document.documentElement.animate(
        {
          clipPath: [clipPathStart, clipPathEnd]
        },
        {
          duration: 500,
          easing: 'ease-out',
          fill: 'both',
          pseudoElement: switchingToDark 
            ? '::view-transition-old(root)' 
            : '::view-transition-new(root)'
        }
      )
    })
  } else {
    // Fallback for browsers without View Transitions API
    isDark.value = !isDark.value
    updateTheme()
  }
}

function updateTheme() {
  const root = window.document.documentElement
  root.classList.remove('light', 'dark')
  root.classList.add(isDark.value ? 'dark' : 'light')
  localStorage.setItem('theme', isDark.value ? 'dark' : 'light')
}

// Language Logic
function setLanguage(lang: string) {
  locale.value = lang
  localStorage.setItem('locale', lang)
  localStorage.removeItem('language')
}

// Account Selection Logic
async function selectAccount(account: ExtractedAccount) {
    selectedAccountId.value = account.user.id
    try {
      await authStore.loginWithToken(account.token)
      authStore.detectedAccounts = [] // Clear after selection
    } finally {
      selectedAccountId.value = null
    }
}

onMounted(() => {
  // Init Theme
  const savedTheme = localStorage.getItem('theme')
  if (savedTheme) {
    isDark.value = savedTheme === 'dark'
  } else {
    isDark.value = window.matchMedia('(prefers-color-scheme: dark)').matches
  }
  updateTheme()

  // Restore debug mode state
  debugModeEnabled.value = localStorage.getItem('debugMode') === 'true'

  // Check for updates
  const versionStore = useVersionStore()
  versionStore.initialize()

  // Listen for tab navigation events from toast actions
  window.addEventListener('app:navigate', handleAppNavigate)
})

onUnmounted(() => {
  window.removeEventListener('app:navigate', handleAppNavigate)
})

function handleAppNavigate(e: Event) {
  const tab = (e as CustomEvent<string>).detail
  if (tab === 'home' || tab === 'game' || tab === 'settings' || tab === 'debug') {
    currentTab.value = tab
  }
}

function handleDebugDisabled() {
  debugModeEnabled.value = false
  if (currentTab.value === 'debug') {
    currentTab.value = 'settings'
  }
}

function openSettingsSection(section: 'discord_integration' | 'quest_behavior' | 'advanced' | 'account') {
  persistSettingsSection(section)
  currentTab.value = 'settings'
}
</script>

<template>
  <div class="h-screen bg-background text-foreground font-sans flex flex-col overflow-hidden">
    <AlertDialog
      :open="cdpLoginRestartDialogOpen"
      @update:open="cdpLoginRestartDialogOpen = $event"
    >
      <AlertDialogContent class="max-w-[520px]">
        <AlertDialogHeader>
          <AlertDialogTitle>{{ t('settings.cdp_dialog_title_disconnected') }}</AlertDialogTitle>
          <AlertDialogDescription>{{ t('settings.cdp_dialog_desc_disconnected') }}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{{ t('dialog.cancel') }}</AlertDialogCancel>
          <AlertDialogAction
            :disabled="cdpLoginPreparing || authStore.loading"
            @click="confirmCdpLoginRestart"
          >
            {{ t('settings.cdp_dialog_confirm') }}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>

    <TitleBar />
    
    <div class="flex-1 overflow-auto">
      <div class="container mx-auto p-6 flex flex-col min-h-full">
      <header class="mb-8 flex flex-col md:flex-row md:items-center justify-between gap-4">
        <div class="flex items-center gap-3">
          <img src="/icons/logo.png" alt="logo" class="w-10 h-10" />
          <div>
            <h1 class="text-3xl font-bold tracking-tight text-primary select-none">
              {{ t('general.title') }}
            </h1>
            <p class="text-muted-foreground select-none">
               {{ t('general.subtitle') }}
            </p>
          </div>
        </div>
        
        <div class="flex items-center gap-2 select-none">
          <QuestModeIndicator
            v-if="authStore.user"
            @open-settings="openSettingsSection('quest_behavior')"
          />

          <!-- Theme Toggle -->
          <Button variant="ghost" size="icon" @click="toggleTheme" :title="t('header.toggle_theme')">
            <Moon v-if="isDark" class="w-5 h-5" />
            <Sun v-else class="w-5 h-5" />
          </Button>

          <!-- Language Toggle -->
          <DropdownMenu>
            <DropdownMenuTrigger as-child>
              <Button variant="ghost" size="icon" :title="t('header.change_language')">
                <Languages class="w-5 h-5" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" class="max-h-[70vh] overflow-y-auto">
              <DropdownMenuItem
                v-for="item in supportedLocales"
                :key="item.code"
                @click="setLanguage(item.code)"
              >
                {{ item.label }}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>

          <AccountMenu v-if="authStore.user" @logout="authStore.logout" />
        </div>
      </header>
      
      <div class="mb-8 flex items-center gap-2 border-b border-border pb-4 select-none">
        <div class="flex gap-2">
          <Button
            :variant="currentTab === 'home' ? 'secondary' : 'ghost'"
            @click="currentTab = 'home'"
          >
            {{ t('nav.home') }}
          </Button>
           <Button
            :variant="currentTab === 'game' ? 'secondary' : 'ghost'"
            @click="currentTab = 'game'"
          >
            {{ t('nav.game_simulator') }}
          </Button>
           <Button
            :variant="currentTab === 'settings' ? 'secondary' : 'ghost'"
            @click="currentTab = 'settings'"
          >
            {{ t('nav.settings') }}
          </Button>
          <Button
            v-if="debugModeEnabled"
            :variant="currentTab === 'debug' ? 'secondary' : 'ghost'"
            @click="currentTab = 'debug'"
          >
            {{ t('nav.debug') }}
          </Button>
        </div>

        <!-- Orbs Balance + Nitro membership status (compact, right-aligned) -->
        <OrbsNitroStatus v-if="authStore.user" />
      </div>
      
      <main class="fade-in flex-1">
        <!-- Home requires login -->
        <template v-if="currentTab === 'home'">
          <Home v-if="authStore.user" :debug-mode-enabled="debugModeEnabled" />
          <!-- Welcome/Login Screen when not logged in -->
          <div v-else class="flex items-center justify-center h-full">
        <div class="max-w-md w-full text-center space-y-8 p-8">
          <div class="space-y-4">
          <img src="/icons/logo.png" alt="logo" class="w-20 h-20 mx-auto opacity-80" />
            <h2 class="text-2xl font-bold">{{ t('general.welcome') }}</h2>
            <p class="text-muted-foreground">
              {{ t('general.login_prompt') }}
            </p>
          </div>
          
          <!-- Account Selection (inline) -->
          <div v-if="authStore.detectedAccounts.length > 0" class="space-y-3">
            <p class="text-sm text-muted-foreground">{{ t('account.select_desc') }}</p>
            <div class="space-y-2 max-h-[200px] overflow-y-auto">
              <Button 
                v-for="account in authStore.detectedAccounts" 
                :key="account.user.id"
                variant="outline"
                class="w-full justify-start h-auto py-3 px-4"
                :disabled="authStore.loading"
                @click="selectAccount(account)"
              >
                <Loader2 
                  v-if="selectedAccountId === account.user.id"
                  class="w-5 h-5 rounded-full mr-3 animate-spin shrink-0"
                />
                <img 
                  v-else-if="account.user.avatar"
                  :src="`https://cdn.discordapp.com/avatars/${account.user.id}/${account.user.avatar}.png`" 
                  class="w-8 h-8 rounded-full mr-3 shrink-0"
                  alt="Avatar"
                />
                <div class="text-left">
                  <div class="font-bold">{{ account.user.global_name || account.user.username }}</div>
                  <div class="text-xs text-muted-foreground">@{{ account.user.username }}</div>
                </div>
              </Button>
            </div>
          </div>
          
          <!-- Login Form (show when no accounts detected) -->
          <div v-else class="space-y-4">
            <Button
              v-if="showAutoDetect"
              size="lg"
              class="w-full gap-2"
              @click="handleAutoDetect"
              :disabled="authStore.loading"
            >
              <Loader2 v-if="authStore.loading" class="w-4 h-4 animate-spin" />
              {{ t('auth.auto_detect') }}
            </Button>

            <!-- CDP auto-login: sign in from the running Discord client, no token
                 needed. Primary path on Linux. -->
            <Button
              size="lg"
              :variant="showAutoDetect ? 'outline' : 'default'"
              class="w-full gap-2"
              @click="handleCdpLogin"
              :disabled="authStore.loading || cdpLoginPreparing"
            >
              <Loader2 v-if="authStore.loading || cdpLoginPreparing" class="w-4 h-4 animate-spin" />
              {{ t('auth.cdp_login') }}
            </Button>
            <p class="text-center text-xs text-muted-foreground">{{ t('auth.cdp_login_hint') }}</p>

            <div class="relative">
              <div class="absolute inset-0 flex items-center">
                <span class="w-full border-t" />
              </div>
              <div class="relative flex justify-center text-xs uppercase">
                <span class="bg-background px-2 text-muted-foreground">{{ t('auth.or_manually') }}</span>
              </div>
            </div>
            
            <div class="flex gap-2">
              <Input 
                v-model="manualTokenInput" 
                type="password" 
                :placeholder="t('auth.enter_token')"
                class="flex-1"
              />
              <Button 
                @click="handleManualLogin" 
                :disabled="!manualTokenInput || authStore.loading"
              >
                {{ t('auth.login') }}
              </Button>
            </div>
            
            <p v-if="authStore.error" class="text-sm text-destructive">{{ authStore.error }}</p>
          </div>
        </div>
          </div>
        </template>
        
        <!-- Game Simulator - no login required -->
        <GameSimulator v-else-if="currentTab === 'game'" />
        
        <!-- Settings - no login required -->
        <Settings 
          v-else-if="currentTab === 'settings'" 
          @navigate-to-home="currentTab = 'home'" 
          @debug-unlocked="debugModeEnabled = true; currentTab = 'debug'"
          @debug-disabled="handleDebugDisabled"
        />
        
        <!-- Debug - no login required -->
        <Debug v-else-if="currentTab === 'debug'" />
      </main>


    </div>
    </div>
    <Toaster />
  </div>
</template>

<style>
/* Global transitions */
.fade-in {
  animation: fadeIn 0.3s ease-in-out;
}

@keyframes fadeIn {
  from { opacity: 0; transform: translateY(5px); }
  to { opacity: 1; transform: translateY(0); }
}
</style>
