<script setup lang="ts">
import { ref } from 'vue'
import { Check, Copy, Download, Loader2 } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'
import { writeTextFile } from '@tauri-apps/plugin-fs'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { useAuthStore } from '@/stores/auth'
import { useQuestsStore } from '@/stores/quests'
import { useVersionStore } from '@/stores/version'

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
  <Card>
    <CardHeader>
      <CardTitle>{{ t('settings.diagnostics') }}</CardTitle>
      <CardDescription>{{ t('settings.diagnostics_desc') }}</CardDescription>
    </CardHeader>
    <CardContent class="space-y-4">
      <p class="text-sm text-muted-foreground">
        {{ t('settings.diagnostics_info') }}
      </p>
      <div class="flex flex-wrap items-center gap-3">
        <Button variant="outline" @click="exportLogs" :disabled="exporting">
          <Download v-if="!exporting" class="mr-2 h-4 w-4" />
          <Loader2 v-else class="mr-2 h-4 w-4 animate-spin" />
          {{ t('settings.export_logs') }}
        </Button>
        <Button variant="outline" @click="copyDiagnosticsSummary">
          <Check v-if="copiedSummary" class="mr-2 h-4 w-4 text-green-500" />
          <Copy v-else class="mr-2 h-4 w-4" />
          {{ t('settings.copy_diagnostics') }}
        </Button>
        <span v-if="exportSuccess" class="flex items-center gap-1 text-sm text-green-500">
          <Check class="h-4 w-4" /> {{ t('settings.export_success') }}
        </span>
        <span v-if="exportError" class="text-sm text-red-500">
          {{ t('settings.export_error') }}
        </span>
      </div>
    </CardContent>
  </Card>
</template>
