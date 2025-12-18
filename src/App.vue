<script setup lang="ts">
import { ref, onMounted } from 'vue'
import UserStatus from './components/UserStatus.vue'
import Home from './views/Home.vue'
import GameSimulator from './views/GameSimulator.vue'
import Settings from './views/Settings.vue'
import TitleBar from './components/TitleBar.vue'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useAuthStore } from '@/stores/auth'
import { useVersionStore } from '@/stores/version'
import type { ExtractedAccount } from '@/api/tauri'
import { useI18n } from 'vue-i18n'
import { Moon, Sun, Loader2, Languages } from 'lucide-vue-next'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

const { t, locale } = useI18n()
const currentTab = ref<'home' | 'game' | 'settings'>('home')
const authStore = useAuthStore()

// Theme Logic
const isDark = ref(true) // Default to dark

// Manual login
const manualTokenInput = ref('')

async function handleManualLogin() {
  if (!manualTokenInput.value) return
  await authStore.loginWithToken(manualTokenInput.value)
  manualTokenInput.value = ''
}

async function handleAutoDetect() {
  await authStore.tryAutoDetect()
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
  localStorage.setItem('language', lang)
}

// Account Selection Logic
async function selectAccount(account: ExtractedAccount) {
    await authStore.loginWithToken(account.token)
    authStore.detectedAccounts = [] // Clear after selection
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
  
  // Check for updates
  const versionStore = useVersionStore()
  versionStore.initialize()
})
</script>

<template>
  <div class="h-screen bg-background text-foreground font-sans flex flex-col overflow-hidden">
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
           <UserStatus v-if="authStore.user" />
           
           <!-- Theme Toggle -->
           <Button variant="ghost" size="icon" @click="toggleTheme" title="Toggle Theme">
             <Moon v-if="isDark" class="w-5 h-5" />
             <Sun v-else class="w-5 h-5" />
           </Button>

           <!-- Language Toggle -->
           <DropdownMenu>
            <DropdownMenuTrigger as-child>
              <Button variant="ghost" size="icon" title="Change Language">
                <Languages class="w-5 h-5" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem @click="setLanguage('en')">English</DropdownMenuItem>
              <DropdownMenuItem @click="setLanguage('zh')">简体中文</DropdownMenuItem>
              <DropdownMenuItem @click="setLanguage('zh-TW')">繁體中文</DropdownMenuItem>
              <DropdownMenuItem @click="setLanguage('ja')">日本語</DropdownMenuItem>
              <DropdownMenuItem @click="setLanguage('ko')">한국어</DropdownMenuItem>
              <DropdownMenuItem @click="setLanguage('ru')">Русский</DropdownMenuItem>
              <DropdownMenuItem @click="setLanguage('es')">Español</DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </header>
      
      <div class="mb-8 flex gap-2 border-b border-border pb-4 select-none">
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
      </div>
      
      <main v-if="authStore.user" class="fade-in flex-1">
        <Home v-if="currentTab === 'home'" />
        <GameSimulator v-else-if="currentTab === 'game'" />
        <Settings v-else-if="currentTab === 'settings'" />
      </main>
      
      <!-- Welcome/Login Screen when not logged in -->
      <main v-else class="fade-in flex-1 flex items-center justify-center">
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
                @click="selectAccount(account)"
              >
                <img 
                  v-if="account.user.avatar"
                  :src="`https://cdn.discordapp.com/avatars/${account.user.id}/${account.user.avatar}.png`" 
                  class="w-8 h-8 rounded-full mr-3"
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
              size="lg" 
              class="w-full gap-2" 
              @click="handleAutoDetect"
              :disabled="authStore.loading"
            >
              <Loader2 v-if="authStore.loading" class="w-4 h-4 animate-spin" />
              {{ t('auth.auto_detect') }}
            </Button>
            
            <div class="relative">
              <div class="absolute inset-0 flex items-center">
                <span class="w-full border-t" />
              </div>
              <div class="relative flex justify-center text-xs uppercase">
                <span class="bg-background px-2 text-muted-foreground">or</span>
              </div>
            </div>
            
            <div class="flex gap-2">
              <Input 
                v-model="manualTokenInput" 
                type="password" 
                :placeholder="t('auth.paste_token') || 'Paste Discord Token'"
                class="flex-1"
              />
              <Button 
                @click="handleManualLogin" 
                :disabled="!manualTokenInput || authStore.loading"
              >
                Login
              </Button>
            </div>
            
            <p v-if="authStore.error" class="text-sm text-destructive">{{ authStore.error }}</p>
          </div>
        </div>
      </main>


    </div>
    </div>
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


