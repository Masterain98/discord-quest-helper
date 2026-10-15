<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import Home from './views/Home.vue'
import GlobalQuests from './views/GlobalQuests.vue'
import GameSimulator from './views/GameSimulator.vue'
import Settings from './views/Settings.vue'
import Debug from './views/Debug.vue'
import DiscordConsole from './views/DiscordConsole.vue'
import TitleBar from './components/TitleBar.vue'
import { Button } from '@/components/ui/button'
import { useAuthStore } from '@/stores/auth'
import { useQuestsStore } from '@/stores/quests'
import { useGlobalQuestsStore } from '@/stores/globalQuests'
import { useGameIdleStore } from '@/stores/gameIdle'
import { useVersionStore } from '@/stores/version'
import { useDiscordConsoleStore } from '@/stores/discordConsole'
import { useToastStore } from '@/stores/toast'
import { checkCdpStatus } from '@/api/tauri'
import { createDiscordConsoleLifecycle } from '@/composables/discordConsoleLifecycle'
import { consoleFallbackTab, consoleTabEnabled } from '@/utils/discordConsole'
import { useI18n } from 'vue-i18n'
import { ArrowUpCircle, ExternalLink, Languages, LogIn, Moon, PanelLeftClose, PanelLeftOpen, Sun, X, Check } from 'lucide-vue-next'
import { DialogContent, DialogOverlay, DialogPortal, DialogRoot, DialogTitle, TooltipProvider, DropdownMenuRadioGroup, DropdownMenuRadioItem } from 'radix-vue'
import { open } from '@tauri-apps/plugin-shell'
import AccountMenu from './components/AccountMenu.vue'
import AppNavigation, { type AppTab } from './components/AppNavigation.vue'
import QuestModeIndicator from './components/QuestModeIndicator.vue'
import OrbsNitroStatus from './components/OrbsNitroStatus.vue'
import QuestProgress from './components/QuestProgress.vue'
import QuestStopDialog from './components/QuestStopDialog.vue'
import Toaster from './components/Toaster.vue'
import DiscordCdpExitDialog from './components/DiscordCdpExitDialog.vue'
import LoginPanel from './components/auth/LoginPanel.vue'
import { useSettingsNavigation, type SettingsSection } from '@/composables/useSettingsNavigation'
import { useSettingsSidebar } from '@/composables/useSettingsSidebar'
import SettingsNav from './components/settings/SettingsNav.vue'
import type { SettingsSearchResult } from '@/composables/useSettingsSearch'
import IconTooltip from './components/IconTooltip.vue'
import IconMenu from './components/IconMenu.vue'
import { supportedLocales } from '@/locales/meta'
import { isDebugModeEnabled } from '@/utils/debugMode'

const { t, locale } = useI18n()
const currentTab = ref<AppTab>('home')
const authStore = useAuthStore()
const versionStore = useVersionStore()
const questsStore = useQuestsStore()
const globalQuestsStore = useGlobalQuestsStore()
const discordConsole = useDiscordConsoleStore()
const toastStore = useToastStore()
const consoleFeatureEnabled = ref(localStorage.getItem('questHelper_discordConsoleEnabled') === 'true')
const consoleAvailable = computed(() => consoleTabEnabled(questsStore.gameQuestMode, questsStore.cdpAvailable))
const consoleEnabled = computed(() => consoleAvailable.value && consoleFeatureEnabled.value)
const consoleLifecycle = createDiscordConsoleLifecycle({
  snapshot: () => ({ mode: questsStore.gameQuestMode, port: questsStore.cdpPort, account: authStore.user?.id ?? null }),
  check: checkCdpStatus,
  setAvailable: available => { questsStore.cdpAvailable = available },
  console: discordConsole,
})
let consoleMonitor: ReturnType<typeof setInterval> | undefined
const closeConsoleOnUnload = () => { void consoleLifecycle.stop() }
watch(() => [questsStore.gameQuestMode, questsStore.cdpPort, authStore.user?.id ?? null] as const,
  async (current, previous) => {
    const accountChanged = !!previous && current[2] !== previous[2]
    if (previous) await consoleLifecycle.invalidate(accountChanged)
    if (previous && current[1] !== previous[1]) questsStore.cdpAvailable = false
    await consoleLifecycle.refresh()
  }, { immediate: true, flush: 'sync' })
watch(() => discordConsole.session, (current, previous) => {
  if (!current && previous && consoleFeatureEnabled.value && questsStore.gameQuestMode === 'cdp') questsStore.cdpAvailable = false
})
watch(consoleAvailable, available => {
  if (!available && currentTab.value === 'console') {
    currentTab.value = consoleFallbackTab(currentTab.value, available)
    toastStore.warning({ title: t('nav.discord_console'), description: t('discord_console.unavailable') })
  }
}, { flush: 'sync' })
watch(() => authStore.user?.id ?? null, id => {
  questsStore.setQuestAccount(id)
  if (id) void questsStore.fetchQuests(true)
}, { immediate: true, flush: 'sync' })
const gameIdleStore = useGameIdleStore()
const immersiveGameIdle = computed(() => currentTab.value === 'game' && (gameIdleStore.isActive || gameIdleStore.loading))
const { selectedSection, setSection, navigationRequest, navigationTarget } = useSettingsNavigation()
const settingsQuery = ref('')
const settingsResults = ref<SettingsSearchResult[]>([])
const { wide, expanded, drawerOpen, toggle: toggleSettingsSidebar } = useSettingsSidebar()
const sidebarToggle = ref<HTMLButtonElement | null>(null)
const sidebarVisible = computed(() => currentTab.value === 'settings' && (wide.value ? expanded.value : drawerOpen.value))
const pageTitle = computed(() => {
  const labels = { home: 'nav.home', global: 'nav.global_quests', game: 'nav.game_simulator', console: 'nav.discord_console', settings: 'nav.settings', debug: 'nav.debug' }
  return currentTab.value === 'home' && !authStore.user ? t('auth.login') : t(labels[currentTab.value])
})
const sidebarToggleLabel = computed(() => t(sidebarVisible.value ? 'settings.navigation_collapse' : 'settings.navigation_expand'))

function selectSettingsSection(section: SettingsSection) {
  setSection(section)
  drawerOpen.value = false
}

function selectSettingsResult(result: SettingsSearchResult) {
  setSection(result.section, result.id)
  drawerOpen.value = false
}

function restoreSidebarFocus(event: Event) {
  event.preventDefault()
  void nextTick(() => {
    const target = sidebarToggle.value ?? document.querySelector<HTMLButtonElement>('.app-rail-button[aria-current="page"]')
    target?.focus({ preventScroll: true })
  })
}

watch([wide, currentTab, immersiveGameIdle], () => { drawerOpen.value = false })

// Theme Logic
const isDark = ref(true) // Default to dark
const themeLabel = computed(() => `${t('header.toggle_theme')} · ${t(isDark.value ? 'sidebar.dark' : 'sidebar.light')}`)
const languageLabel = computed(() => `${t('header.change_language')} · ${supportedLocales.find(item => item.code === locale.value)?.label ?? locale.value}`)

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
  globalQuestsStore.initialize()
  consoleMonitor = setInterval(() => { void consoleLifecycle.refresh() }, 5000)
  window.addEventListener('beforeunload', closeConsoleOnUnload)
  // Init Theme
  const savedTheme = localStorage.getItem('theme')
  if (savedTheme) {
    isDark.value = savedTheme === 'dark'
  } else {
    isDark.value = window.matchMedia('(prefers-color-scheme: dark)').matches
  }
  updateTheme()

  // Restore debug mode state
  debugModeEnabled.value = isDebugModeEnabled()

  // Check for updates
  versionStore.initialize()

  // Listen for tab navigation events from toast actions
  window.addEventListener('app:navigate', handleAppNavigate)
})

onUnmounted(() => {
  if (consoleMonitor) clearInterval(consoleMonitor)
  window.removeEventListener('beforeunload', closeConsoleOnUnload)
  void consoleLifecycle.stop()
  window.removeEventListener('app:navigate', handleAppNavigate)
})

function handleAppNavigate(e: Event) {
  const tab = (e as CustomEvent<string>).detail
  if (tab === 'console' && consoleAvailable.value) currentTab.value = tab
  if (tab === 'home' || tab === 'global' || tab === 'game' || tab === 'settings' || (tab === 'debug' && debugModeEnabled.value)) {
    currentTab.value = tab
  }
}

function setConsoleFeatureEnabled(enabled: boolean) {
  consoleFeatureEnabled.value = enabled
  try { localStorage.setItem('questHelper_discordConsoleEnabled', String(enabled)) } catch { /* Keep the in-memory preference. */ }
  if (!enabled) void consoleLifecycle.invalidate(true)
}

function handleDebugDisabled() {
  debugModeEnabled.value = false
  if (currentTab.value === 'debug') {
    currentTab.value = 'settings'
  }
}

async function openUpdatePage() {
  if (versionStore.latestRelease?.html_url) {
    await open(versionStore.latestRelease.html_url)
  }
}

watch(
  () => Boolean(authStore.user),
  (authenticated, wasAuthenticated) => {
    if (authenticated === wasAuthenticated || currentTab.value !== 'home') return

    const prefersReducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    if (!document.startViewTransition || prefersReducedMotion) {
      return
    }

    const transition = document.startViewTransition(async () => {
      await nextTick()
    })

    void transition.finished.catch(() => undefined)
  },
  { flush: 'sync' },
)
</script>

<template>
  <TooltipProvider :delay-duration="200" :ignore-non-keyboard-focus="true">
    <DialogRoot :open="!wide && drawerOpen && currentTab === 'settings' && !immersiveGameIdle" @update:open="drawerOpen = $event">
      <div class="flex h-dvh flex-col overflow-hidden bg-background font-sans text-foreground">
        <DiscordCdpExitDialog />
        <TitleBar :brand-visible="!immersiveGameIdle" />
        <div class="app-layout" :class="{ 'app-layout-immersive': immersiveGameIdle }">
          <aside v-show="!immersiveGameIdle" class="app-icon-rail">
            <AppNavigation :current="currentTab" :debug-enabled="debugModeEnabled" :console-enabled="consoleAvailable" :suspended="immersiveGameIdle" @navigate="currentTab = $event">
              <div class="app-rail-tools" role="group" :aria-label="t('sidebar.utilities')">
                <QuestProgress v-if="authStore.user" />
                <div class="app-rail-tool-group">
                  <QuestModeIndicator v-if="authStore.user" :suspended="immersiveGameIdle" />
                  <OrbsNitroStatus v-if="authStore.user" :suspended="immersiveGameIdle" />
                </div>
                <span v-if="authStore.user" class="app-rail-divider" aria-hidden="true" />
                <div class="app-rail-display-actions">
                  <IconTooltip :label="themeLabel" :disabled="immersiveGameIdle">
                    <button type="button" class="app-rail-button rail-tool-button" :aria-label="themeLabel" :aria-pressed="isDark" @click="toggleTheme">
                      <Moon v-if="isDark" class="h-4 w-4" aria-hidden="true" />
                      <Sun v-else class="h-4 w-4" aria-hidden="true" />
                    </button>
                  </IconTooltip>
                  <IconMenu :label="languageLabel" side="right" :suspended="immersiveGameIdle" content-class="rail-language-menu">
                    <template #trigger>
                      <button type="button" class="app-rail-button rail-tool-button rail-language-trigger" :aria-label="languageLabel">
                        <Languages class="h-4 w-4" aria-hidden="true" />
                      </button>
                    </template>
                    <div class="rail-language-heading">
                      <span class="rail-panel-glyph"><Languages class="h-4 w-4" aria-hidden="true" /></span>
                      <span class="min-w-0 text-sm font-semibold">{{ t('header.change_language') }}</span>
                    </div>
                    <DropdownMenuRadioGroup :model-value="locale" @update:model-value="setLanguage">
                      <DropdownMenuRadioItem v-for="item in supportedLocales" :key="item.code" :value="item.code" :aria-label="item.label" class="rail-language-option">
                        <span class="flex-1">{{ item.label }}</span>
                        <Check v-if="locale === item.code" class="h-4 w-4 shrink-0 text-primary" aria-hidden="true" />
                      </DropdownMenuRadioItem>
                    </DropdownMenuRadioGroup>
                  </IconMenu>
                </div>
              </div>
            </AppNavigation>
            <AccountMenu v-if="authStore.user" compact :suspended="immersiveGameIdle" @logout="authStore.logout" />
            <IconTooltip v-else :label="t('auth.login')" :disabled="immersiveGameIdle">
              <button type="button" class="app-rail-button" :aria-label="t('auth.login')" @click="currentTab = 'home'">
                <LogIn class="h-4 w-4" :stroke-width="1.7" aria-hidden="true" />
              </button>
            </IconTooltip>
          </aside>

          <aside v-if="wide && expanded && currentTab === 'settings' && !immersiveGameIdle" id="settings-navigation-panel" class="app-settings-panel">
            <div class="flex h-12 shrink-0 items-center border-b border-border/50 px-5 text-sm font-semibold">{{ t('settings.title') }}</div>
            <div class="min-h-0 flex-1 overflow-y-auto py-2">
              <SettingsNav v-model:query="settingsQuery" :results="settingsResults" :selected="selectedSection" @update:selected="selectSettingsSection" @select-result="selectSettingsResult" />
            </div>
          </aside>

          <section class="app-workspace" :class="{ 'app-workspace-immersive': immersiveGameIdle }" :aria-label="pageTitle">
            <header v-show="currentTab === 'settings' && !immersiveGameIdle" class="app-workspace-toolbar">
              <div class="flex min-w-0 items-center gap-2">
                <IconTooltip v-if="currentTab === 'settings'" :label="sidebarToggleLabel" side="bottom">
                  <button ref="sidebarToggle" type="button" class="app-toolbar-button" :aria-label="sidebarToggleLabel" :aria-expanded="sidebarVisible" aria-controls="settings-navigation-panel" @click="toggleSettingsSidebar">
                    <PanelLeftClose v-if="sidebarVisible" class="h-4 w-4" aria-hidden="true" />
                    <PanelLeftOpen v-else class="h-4 w-4" aria-hidden="true" />
                  </button>
                </IconTooltip>
              </div>
            </header>

            <main class="app-workspace-scroll" tabindex="-1">
              <div class="app-content" :class="{ 'app-content-immersive': immersiveGameIdle, 'app-content-login': currentTab === 'home' && !authStore.user, 'app-content-game': currentTab === 'game' }">
                <div v-if="!immersiveGameIdle && versionStore.hasUpdate && versionStore.latestRelease" role="status" class="mb-6 flex shrink-0 flex-col items-start justify-between gap-3 rounded-xl border border-primary/30 bg-primary/10 p-4 sm:flex-row sm:items-center">
                  <div class="flex items-center gap-3">
                    <ArrowUpCircle class="h-5 w-5 shrink-0 text-primary" />
                    <div>
                      <p class="font-semibold text-primary">{{ t('version.update_available') }}</p>
                      <p class="text-sm text-muted-foreground">{{ t('version.update_desc', { version: versionStore.latestRelease.tag_name, current: versionStore.currentVersion }) }}</p>
                    </div>
                  </div>
                  <Button @click="openUpdatePage" class="shrink-0 gap-2"><ExternalLink class="h-4 w-4" />{{ t('version.download') }}</Button>
                </div>
                <template v-if="currentTab === 'home'">
                  <Home v-if="authStore.user" :debug-mode-enabled="debugModeEnabled" />
                  <LoginPanel v-else />
                </template>
                <GlobalQuests v-else-if="currentTab === 'global'" />
                <GameSimulator v-else-if="currentTab === 'game'" />
                <Settings v-else-if="currentTab === 'settings'" :selected-section="selectedSection" :navigation-request="navigationRequest" :navigation-target="navigationTarget" :search-query="settingsQuery" @search-results="settingsResults = $event" @update:selected-section="selectedSection = $event" @navigate-to-home="currentTab = 'home'" @debug-unlocked="debugModeEnabled = true; currentTab = 'debug'" @debug-disabled="handleDebugDisabled" />
                <Debug v-else-if="currentTab === 'debug'" />
                <DiscordConsole v-if="consoleAvailable" v-show="currentTab === 'console'" :active="currentTab === 'console'" :enabled="consoleEnabled" :port="questsStore.cdpPort" @enable="setConsoleFeatureEnabled(true)" @disable="setConsoleFeatureEnabled(false)" />
              </div>
            </main>
          </section>
        </div>
        <Toaster />
        <QuestStopDialog />
      </div>

      <DialogPortal>
        <DialogOverlay class="app-settings-overlay" @click="drawerOpen = false" />
        <DialogContent id="settings-navigation-panel" class="ui-dialog app-settings-drawer" :aria-describedby="undefined" @pointer-down-outside.prevent @close-auto-focus="restoreSidebarFocus">
          <div class="flex h-12 shrink-0 items-center justify-between border-b border-border/50 px-4">
            <DialogTitle class="text-sm font-semibold">{{ t('settings.title') }}</DialogTitle>
            <button type="button" class="app-toolbar-button" :aria-label="t('settings.navigation_close')" @click="drawerOpen = false"><X class="h-4 w-4" aria-hidden="true" /></button>
          </div>
          <div class="min-h-0 flex-1 overflow-y-auto py-2"><SettingsNav v-model:query="settingsQuery" :results="settingsResults" :selected="selectedSection" @update:selected="selectSettingsSection" @select-result="selectSettingsResult" /></div>
        </DialogContent>
      </DialogPortal>
    </DialogRoot>
  </TooltipProvider>
</template>
<style>
/* Global transitions */
.app-brand-lockup {
  view-transition-name: none;
}

html.account-view-transition .app-brand-lockup {
  view-transition-name: app-rail-brand;
}

html.account-view-transition .login-card-shell {
  view-transition-name: login-card;
}

html.account-view-transition .app-workspace-toolbar {
  view-transition-name: app-toolbar;
}

::view-transition-group(app-rail-brand) {
  z-index: 20;
  animation-duration: 680ms;
  animation-timing-function: cubic-bezier(0.22, 1, 0.36, 1);
}

::view-transition-old(app-rail-brand),
::view-transition-new(app-rail-brand) {
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
::view-transition-group(app-toolbar) {
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
::view-transition-old(app-toolbar),
::view-transition-new(app-toolbar) {
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
  .fade-in {
    animation-duration: 1ms;
    transition-duration: 1ms;
  }
}
</style>
