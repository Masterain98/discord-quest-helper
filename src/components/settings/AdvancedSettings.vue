<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { Loader2, RotateCw, SlidersHorizontal } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useQuestsStore } from '@/stores/quests'
import { useSuperPropertiesDiagnostics } from '@/composables/superPropertiesDiagnostics'
import SettingRow from './SettingRow.vue'
import SettingsSectionCard from './SettingsSectionCard.vue'
import { cn } from '@/lib/utils'
import { settingToneClass, type SettingsTone } from './settingTones'
import { isDebugModeEnabled } from '@/utils/debugMode'

const { t } = useI18n()
const props = defineProps<{ debugModeEnabled?: boolean }>()
const questsStore = useQuestsStore()

// Apply the completed port once, without probing each intermediate digit.
function commitCdpPort(event: FocusEvent) {
  const input = event.target as HTMLInputElement
  if (input.value !== '' && input.validity.valid && Number.isInteger(input.valueAsNumber)) {
    questsStore.cdpPort = input.valueAsNumber
  }
  input.value = String(questsStore.cdpPort)
}

const { superPropsMode, loadSuperPropsMode: readSuperPropsMode, retrySuperPropsMode } = useSuperPropertiesDiagnostics()
const retryingMode = ref(false)
const debugModeEnabled = ref(isDebugModeEnabled())
watch(() => props.debugModeEnabled, enabled => {
  if (enabled !== undefined) debugModeEnabled.value = enabled
}, { immediate: true })

const superPropsTone = computed<SettingsTone>(() => {
  if (superPropsMode.value?.mode === 'cdp') return 'success'
  if (superPropsMode.value?.mode === 'remote_js') return 'warning'
  return 'danger'
})

const developerModeTone = computed<SettingsTone>(() => debugModeEnabled.value ? 'success' : 'neutral')

async function loadSuperPropsMode() {
  try {
    await readSuperPropsMode()
  } catch (e) {
    console.error('Failed to get SuperProperties mode:', e)
  }
}

async function retrySuperProps() {
  retryingMode.value = true
  try {
    await retrySuperPropsMode(questsStore.cdpPort)
  } catch (e) {
    console.error('Retry failed:', e)
  } finally {
    retryingMode.value = false
  }
}

onMounted(async () => {
  debugModeEnabled.value = isDebugModeEnabled()
  await loadSuperPropsMode()
})
</script>

<template>
  <SettingsSectionCard
    :title="t('settings.nav_advanced')"
    :description="t('settings.advanced_desc')"
    :icon="SlidersHorizontal"
    tone="warning"
    content-class="space-y-5"
  >
      <div class="ui-recess rounded-xl border border-border/60 px-4">
        <SettingRow :label="t('settings.cdp_port')" :description="`${t('settings.cdp_port_hint')} · ${t('settings.custom_port_desc')}`">
          <Input
            type="number"
            :model-value="questsStore.cdpPort"
            min="1024"
            max="65535"
            step="1"
            :aria-label="t('settings.cdp_port')"
            class="w-32 max-w-full font-mono"
            @blur="commitCdpPort"
            @keydown.enter="($event.target as HTMLInputElement).blur()"
          />
        </SettingRow>

        <SettingRow :label="t('settings.super_props_mode')" :description="t('settings.super_props_mode_desc')">
          <div class="flex flex-wrap items-center gap-2">
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
