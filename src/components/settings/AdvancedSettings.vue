<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue'
import { Loader2, RotateCw, SlidersHorizontal } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useQuestsStore } from '@/stores/quests'
import { getSuperPropertiesMode, retrySuperProperties, type SuperPropertiesModeInfo } from '@/api/tauri'
import SettingRow from './SettingRow.vue'
import { navigateToTab } from '@/utils/navigate'
import SettingsSectionCard from './SettingsSectionCard.vue'
import { cn } from '@/lib/utils'
import { settingToneClass, type SettingsTone } from './settingTones'

function goToPortSection() {
  navigateToTab('settings', 'discord_integration')
  nextTick(() => {
    setTimeout(() => {
      document.getElementById('custom-port-section')?.scrollIntoView({ behavior: 'smooth', block: 'center' })
    }, 100)
  })
}

const { t } = useI18n()
const questsStore = useQuestsStore()

const superPropsMode = ref<SuperPropertiesModeInfo | null>(null)
const retryingMode = ref(false)
const debugModeEnabled = ref(localStorage.getItem('debugMode') === 'true')

const superPropsTone = computed<SettingsTone>(() => {
  if (superPropsMode.value?.mode === 'cdp') return 'success'
  if (superPropsMode.value?.mode === 'remote_js') return 'warning'
  return 'danger'
})

const developerModeTone = computed<SettingsTone>(() => debugModeEnabled.value ? 'success' : 'neutral')

async function loadSuperPropsMode() {
  try {
    superPropsMode.value = await getSuperPropertiesMode()
  } catch (e) {
    console.error('Failed to get SuperProperties mode:', e)
  }
}

async function retrySuperProps() {
  retryingMode.value = true
  try {
    await retrySuperProperties(questsStore.cdpPort)
    await loadSuperPropsMode()
  } catch (e) {
    console.error('Retry failed:', e)
  } finally {
    retryingMode.value = false
  }
}

onMounted(async () => {
  debugModeEnabled.value = localStorage.getItem('debugMode') === 'true'
  await loadSuperPropsMode()
})
</script>

<template>
  <SettingsSectionCard
    :title="t('settings.advanced_title')"
    :description="t('settings.advanced_desc')"
    :icon="SlidersHorizontal"
    tone="warning"
    content-class="space-y-5"
  >
      <div class="rounded-lg border px-4">
        <SettingRow :label="t('settings.cdp_port')" :description="t('settings.cdp_port_hint')">
          <div class="flex items-center gap-2">
            <Badge variant="outline" class="border-primary/40 bg-primary/10 font-mono text-primary">
              {{ questsStore.cdpPort }}
            </Badge>
            <Button
              variant="outline"
              size="sm"
              :class="settingToneClass.primary.buttonSoft"
              @click="goToPortSection"
            >
              {{ t('settings.edit_port') }}
            </Button>
          </div>
        </SettingRow>

        <SettingRow :label="t('settings.super_props_mode')" :description="t('settings.super_props_mode_desc')">
          <div class="flex items-center gap-2">
            <Badge
              variant="outline"
              :class="settingToneClass[superPropsTone].badge"
            >
              {{ superPropsMode?.mode === 'cdp' ? 'CDP' : (superPropsMode?.mode === 'remote_js' ? t('settings.remote_js') : t('settings.default_mode')) }}
            </Badge>
            <Button
              variant="outline"
              size="sm"
              :aria-label="t('settings.super_props_mode_desc')"
              :class="cn('h-7 gap-1 px-2', settingToneClass.info.buttonSoft)"
              @click="retrySuperProps"
              :disabled="retryingMode"
            >
              <Loader2 v-if="retryingMode" class="h-3 w-3 animate-spin" />
              <RotateCw v-else class="h-3 w-3" />
            </Button>
          </div>
        </SettingRow>

        <SettingRow :label="t('settings.developer_mode')" :description="t('settings.developer_mode_desc')">
          <Badge variant="outline" :class="settingToneClass[developerModeTone].badge">
            {{ debugModeEnabled ? t('settings.debug_already_unlocked') : t('settings.developer_mode_locked') }}
          </Badge>
        </SettingRow>
      </div>

  </SettingsSectionCard>
</template>
