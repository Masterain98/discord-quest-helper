<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import Home from './views/Home.vue'
import GameSimulator from './views/GameSimulator.vue'
import Settings from './views/Settings.vue'
import Debug from './views/Debug.vue'
import TitleBar from './components/TitleBar.vue'
import { Button } from '@/components/ui/button'
import { useAuthStore } from '@/stores/auth'
import { useVersionStore } from '@/stores/version'
import { useI18n } from 'vue-i18n'
import { Moon, Sun, Languages } from 'lucide-vue-next'
import AccountMenu from './components/AccountMenu.vue'
import AppNavigation, { type AppTab } from './components/AppNavigation.vue'
import QuestModeIndicator from './components/QuestModeIndicator.vue'
import Toaster from './components/Toaster.vue'
import DiscordCdpExitDialog from './components/DiscordCdpExitDialog.vue'
import LoginPanel from './components/auth/LoginPanel.vue'
import { persistSettingsSection } from '@/composables/useSettingsNavigation'
import { supportedLocales } from '@/locales/meta'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

const { t, locale } = useI18n()
const currentTab = ref<AppTab>('home')
const authStore = useAuthStore()
const authTransitioning = ref(false)
const showStandardShell = computed(() => Boolean(authStore.user) || currentTab.value !== 'home')

// Theme Logic
const isDark = ref(true) // Default to dark

// Debug mode state
const debugModeEnabled = ref(false)



function toggleTheme(event: MouseEvent) {
  const root = document.documentElement
  const triggerRect = (event.currentTarget as HTMLElement | null)?.getBoundingClientRect()
  const x = event.clientX || (triggerRect ? triggerRect.left + triggerRect.width / 2 : window.innerWidth / 2)
  const y = event.clientY || (triggerRect ? triggerRect.top + triggerRect.height / 2 : window.innerHeight / 2)
  const endRadius = Math.hypot(
    Math.max(x, window.innerWidth - x),
    Math.max(y, window.innerHeight - y)
  )

  const applyNextTheme = () => {
    isDark.value = !isDark.value
    updateTheme()
  }

  const prefersReducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches
  if (!document.startViewTransition || prefersReducedMotion) {
    applyNextTheme()
    return
  }

  // Ignore overlapping toggles so an earlier transition cannot clean up the
  // coordinates or stacking context used by a newer one.
  if (root.classList.contains('theme-view-transition')) return

  root.style.setProperty('--theme-transition-x', `${x}px`)
  root.style.setProperty('--theme-transition-y', `${y}px`)
  root.classList.add('theme-view-transition')

  const transition = document.startViewTransition(applyNextTheme)

  void transition.ready.then(() => {
    root.animate(
      {
        clipPath: [
          `circle(0px at ${x}px ${y}px)`,
          `circle(${endRadius}px at ${x}px ${y}px)`,
        ],
      },
      {
        duration: 500,
        easing: 'ease-out',
        fill: 'both',
        pseudoElement: '::view-transition-new(root)',
      }
    )
  }).catch(() => undefined)

  const cleanUpTransition = () => {
    root.classList.remove('theme-view-transition')
    root.style.removeProperty('--theme-transition-x')
    root.style.removeProperty('--theme-transition-y')
  }

  void transition.finished.then(cleanUpTransition, cleanUpTransition)
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

watch(
  () => Boolean(authStore.user),
  (authenticated, wasAuthenticated) => {
    if (authenticated === wasAuthenticated || currentTab.value !== 'home') return

    const prefersReducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    if (!document.startViewTransition || prefersReducedMotion) {
      authTransitioning.value = false
      return
    }

    authTransitioning.value = true
    const transition = document.startViewTransition(async () => {
      await nextTick()
    })

    transition.finished.finally(() => {
      authTransitioning.value = false
    })
  },
  { flush: 'sync' },
)
</script>

<template>
  <div class="h-screen bg-background text-foreground font-sans flex flex-col overflow-hidden">
    <DiscordCdpExitDialog />
    <TitleBar />
    
    <div class="flex-1 overflow-auto">
      <div
        :class="[
          'container mx-auto flex min-h-full flex-col',
          showStandardShell ? 'p-6' : 'px-4 py-3 sm:px-6',
        ]"
      >
        <Transition name="shell-reveal" appear>
          <header
            v-if="showStandardShell && !authTransitioning"
            class="app-navbar mb-8 p-3 select-none"
          >
            <div class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3">
              <div class="app-brand-lockup flex min-w-0 items-center gap-3 px-1">
                <img
                  src="/icons/logo.png"
                  :alt="t('general.title')"
                  class="h-10 w-10 shrink-0"
                />
                <div class="min-w-0">
                  <h1 class="whitespace-nowrap text-lg font-semibold tracking-tight text-foreground">
                    {{ t('general.title') }}
                  </h1>
                  <p class="hidden max-w-[36rem] truncate text-xs text-muted-foreground sm:block">
                    {{ t('general.subtitle') }}
                  </p>
                </div>
              </div>

              <div class="flex shrink-0 items-center justify-end gap-1">
                <Button
                  variant="ghost"
                  size="icon"
                  class="h-9 w-9"
                  @click="toggleTheme"
                  :title="t('header.toggle_theme')"
                >
                  <Moon v-if="isDark" class="h-4 w-4" />
                  <Sun v-else class="h-4 w-4" />
                </Button>

                <DropdownMenu>
                  <DropdownMenuTrigger as-child>
                    <Button
                      variant="ghost"
                      size="icon"
                      class="h-9 w-9"
                      :title="t('header.change_language')"
                    >
                      <Languages class="h-4 w-4" />
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
            </div>

            <div class="mt-2 grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 border-t border-border/55 pt-2">
              <AppNavigation
                :current="currentTab"
                :debug-enabled="debugModeEnabled"
                @navigate="currentTab = $event"
              />

              <QuestModeIndicator
                v-if="authStore.user"
                class="justify-self-end"
                @open-settings="openSettingsSection('quest_behavior')"
              />
            </div>
          </header>
        </Transition>

        <main :class="['fade-in flex-1', !showStandardShell && 'flex min-h-0 w-full']">
          <template v-if="currentTab === 'home'">
            <Home v-if="authStore.user" :debug-mode-enabled="debugModeEnabled" />
            <LoginPanel v-else>
              <template #toolbar>
                <div class="login-toolbar select-none">
                  <div class="flex flex-wrap items-center justify-center gap-1">
                    <AppNavigation
                      :current="currentTab"
                      :debug-enabled="debugModeEnabled"
                      @navigate="currentTab = $event"
                    />

                    <span class="mx-1 hidden h-5 w-px bg-border sm:block" aria-hidden="true" />

                    <Button variant="ghost" size="icon" class="h-9 w-9" @click="toggleTheme" :title="t('header.toggle_theme')">
                      <Moon v-if="isDark" class="h-4 w-4" />
                      <Sun v-else class="h-4 w-4" />
                    </Button>

                    <DropdownMenu>
                      <DropdownMenuTrigger as-child>
                        <Button variant="ghost" size="icon" class="h-9 w-9" :title="t('header.change_language')">
                          <Languages class="h-4 w-4" />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="center" class="max-h-[70vh] overflow-y-auto">
                        <DropdownMenuItem
                          v-for="item in supportedLocales"
                          :key="item.code"
                          @click="setLanguage(item.code)"
                        >
                          {{ item.label }}
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                </div>
              </template>
            </LoginPanel>
          </template>
        
          <GameSimulator v-else-if="currentTab === 'game'" />
        
          <Settings
            v-else-if="currentTab === 'settings'"
            @navigate-to-home="currentTab = 'home'"
            @debug-unlocked="debugModeEnabled = true; currentTab = 'debug'"
            @debug-disabled="handleDebugDisabled"
          />
        
          <Debug v-else-if="currentTab === 'debug'" />
        </main>
      </div>
    </div>
    <Toaster />
  </div>
</template>

<style>
/* Global transitions */
.app-brand-lockup {
  view-transition-name: app-brand;
}

html.account-view-transition .login-brand-stage {
  view-transition-name: login-brand !important;
}

html.account-view-transition .login-toolbar {
  view-transition-name: login-toolbar;
}

html.account-view-transition .login-card-shell {
  view-transition-name: login-card;
}

.login-toolbar,
.app-navbar {
  max-width: 100%;
  border: 1px solid hsl(var(--border) / 0.58);
  border-radius: 0.875rem;
  background: hsl(var(--card) / 0.64);
  box-shadow:
    inset 0 1px 0 hsl(var(--background) / 0.5),
    0 12px 28px -26px hsl(var(--foreground) / 0.38);
  backdrop-filter: blur(14px);
}

.login-toolbar {
  padding: 0.375rem;
}

html.account-view-transition .app-navbar {
  view-transition-name: login-toolbar;
}

.shell-reveal-enter-active,
.shell-reveal-leave-active {
  transition:
    opacity 260ms ease,
    transform 420ms cubic-bezier(0.22, 1, 0.36, 1);
}

.shell-reveal-enter-from,
.shell-reveal-leave-to {
  opacity: 0;
  transform: translateY(-0.5rem);
}

::view-transition-group(app-brand) {
  z-index: 20;
  animation-duration: 680ms;
  animation-timing-function: cubic-bezier(0.22, 1, 0.36, 1);
}

::view-transition-old(app-brand),
::view-transition-new(app-brand) {
  height: 100%;
  mix-blend-mode: normal;
}

html.account-view-transition::view-transition-old(root) {
  animation: none !important;
  display: none;
  mix-blend-mode: normal;
  opacity: 0 !important;
}

html.account-view-transition::view-transition-new(root) {
  animation: none !important;
  mix-blend-mode: normal;
  opacity: 1 !important;
}

::view-transition-group(login-brand),
::view-transition-group(login-toolbar) {
  animation-duration: 760ms;
  animation-timing-function: cubic-bezier(0.22, 1, 0.36, 1);
}

::view-transition-group(login-card) {
  animation-duration: 760ms;
  animation-timing-function: cubic-bezier(0.16, 1, 0.3, 1);
  perspective: 70rem;
}

::view-transition-old(login-brand),
::view-transition-new(login-brand),
::view-transition-old(login-toolbar),
::view-transition-new(login-toolbar) {
  mix-blend-mode: normal;
}

::view-transition-old(login-card) {
  animation: loginCardOut 440ms cubic-bezier(0.4, 0, 0.8, 1) both;
  mix-blend-mode: normal;
  transform-origin: center;
  will-change: filter, opacity, transform;
}

::view-transition-new(login-card) {
  animation: loginCardIn 590ms 140ms cubic-bezier(0.16, 1, 0.3, 1) both;
  mix-blend-mode: normal;
  transform-origin: center;
  will-change: filter, opacity, transform;
}

.fade-in {
  animation: fadeIn 0.3s ease-in-out;
}

@keyframes fadeIn {
  from { opacity: 0; transform: translateY(5px); }
  to { opacity: 1; transform: translateY(0); }
}

@keyframes loginCardOut {
  from {
    opacity: 1;
    filter: blur(0) brightness(1);
    transform: perspective(70rem) translate3d(0, 0, 0) rotateY(0);
  }
  to {
    opacity: 0;
    filter: blur(1.5px) brightness(0.96);
    transform: perspective(70rem) translate3d(-3.75rem, 0, -5.5rem) rotateY(2.5deg);
  }
}

@keyframes loginCardIn {
  from {
    opacity: 0;
    filter: blur(1.5px) brightness(0.96);
    transform: perspective(70rem) translate3d(3.75rem, 0, -5.5rem) rotateY(-2.5deg);
  }
  to {
    opacity: 1;
    filter: blur(0) brightness(1);
    transform: perspective(70rem) translate3d(0, 0, 0) rotateY(0);
  }
}

@media (prefers-reduced-motion: reduce) {
  .fade-in,
  .shell-reveal-enter-active,
  .shell-reveal-leave-active {
    animation-duration: 1ms;
    transition-duration: 1ms;
  }
}
</style>
