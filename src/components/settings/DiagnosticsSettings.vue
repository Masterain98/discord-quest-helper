<script setup lang="ts">
import { ref } from 'vue'
import { Check, Copy, Download, Info, Loader2, Stethoscope } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'
import { writeTextFile } from '@tauri-apps/plugin-fs'
import { Button } from '@/components/ui/button'
import { useAuthStore } from '@/stores/auth'
import { useQuestsStore } from '@/stores/quests'
import { useVersionStore } from '@/stores/version'
import SettingsSectionCard from './SettingsSectionCard.vue'
import SettingsStatusPanel from './SettingsStatusPanel.vue'
import { cn } from '@/lib/utils'
import { settingToneClass } from './settingTones'

const { t } = useI18n()
const authStore = useAuthStore()
const questsStore = useQuestsStore()
const versionStore = useVersionStore()

const exporting = ref(false)
const exportSuccess = ref(false)
const exportError = ref(false)
const copiedSummary = ref(false)

async function exportLogs() {
  exporting.value = true
  exportSuccess.value = false
  exportError.value = false
  try {
    const logs = await invoke<string>('export_logs')
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)
    const path = await save({
      filters: [{ name: 'JSON', extensions: ['json'] }],
      defaultPath: `dqh-logs-${timestamp}.json`,
    })
    if (!path) return
    await writeTextFile(path, logs)
    exportSuccess.value = true
    setTimeout(() => { exportSuccess.value = false }, 3000)
  } catch (error) {
    console.error('Failed to export logs:', error)
    exportError.value = true
    setTimeout(() => { exportError.value = false }, 5000)
  } finally {
    exporting.value = false
  }
}

async function copyDiagnosticsSummary() {
  const summary = [
    `Version: ${versionStore.currentVersion}`,
    `Account: ${authStore.user ? authStore.user.username : 'not connected'}`,
    `Quest mode: ${questsStore.gameQuestMode}`,
    `CDP available: ${questsStore.cdpAvailable}`,
    `Active quest: ${questsStore.activeQuestId ?? 'none'}`,
    `Queue length: ${questsStore.questQueue.length}`,
    `Quest count: ${questsStore.quests.length}`,
  ].join('\n')

  await navigator.clipboard.writeText(summary)
  copiedSummary.value = true
  setTimeout(() => { copiedSummary.value = false }, 2000)
}
</script>

<template>
  <SettingsSectionCard
    :title="t('settings.diagnostics')"
    :description="t('settings.diagnostics_desc')"
    :icon="Stethoscope"
    tone="info"
  >
      <SettingsStatusPanel tone="info" :icon="Info">
        {{ t('settings.diagnostics_info') }}
      </SettingsStatusPanel>
      <div class="flex flex-wrap items-center gap-3">
        <Button
          variant="outline"
          :class="cn('gap-2', settingToneClass.info.buttonSoft)"
          @click="exportLogs"
          :disabled="exporting"
        >
          <Download v-if="!exporting" class="h-4 w-4" />
          <Loader2 v-else class="h-4 w-4 animate-spin" />
          {{ t('settings.export_logs') }}
        </Button>
        <Button
          variant="outline"
          :class="cn('gap-2', settingToneClass.primary.buttonSoft)"
          @click="copyDiagnosticsSummary"
        >
          <Check v-if="copiedSummary" class="h-4 w-4" />
          <Copy v-else class="h-4 w-4" />
          {{ t('settings.copy_diagnostics') }}
        </Button>
      </div>
      <SettingsStatusPanel v-if="exportSuccess" tone="success" :icon="Check">
        {{ t('settings.export_success') }}
      </SettingsStatusPanel>
      <SettingsStatusPanel v-if="exportError" tone="danger">
          {{ t('settings.export_error') }}
      </SettingsStatusPanel>
  </SettingsSectionCard>
</template>
