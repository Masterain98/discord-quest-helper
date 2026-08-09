<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { message } from '@tauri-apps/plugin-dialog'
import { useI18n } from 'vue-i18n'
import { LoaderCircle } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import {
  AlertDialog,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import {
  createAppExitGuard,
  type ExitGuardState,
  type RunningCdpSession,
} from '@/composables/appExitGuard'
import {
  listRunningDiscordCdpSessions,
  exitAppNow,
  prepareAppExit,
  startDiscordNormalRestoreHelper,
} from '@/api/tauri'

const { t } = useI18n()
const appWindow = getCurrentWindow()
const state = ref<ExitGuardState>({ checking: false, dialogOpen: false, closing: false })
let unlisten: UnlistenFn | undefined

const guard = createAppExitGuard({
  listSessions: () => listRunningDiscordCdpSessions() as Promise<RunningCdpSession[]>,
  startRestoreHelper: startDiscordNormalRestoreHelper,
  prepareExit: prepareAppExit,
  exitApplication: exitAppNow,
  showError: async error => {
    await message(`${t('exit_cdp.helper_error')}\n\n${error}`, {
      title: t('exit_cdp.title'),
      kind: 'error',
    })
  },
  logError: error => console.error('[app-exit]', error),
  onStateChange: next => { state.value = next },
})

onMounted(async () => {
  unlisten = await appWindow.onCloseRequested(event => guard.requestClose(event))
})

onUnmounted(() => unlisten?.())
</script>

<template>
  <AlertDialog :open="state.dialogOpen">
    <AlertDialogContent class="max-w-[560px]">
      <AlertDialogHeader>
        <AlertDialogTitle>{{ t('exit_cdp.title') }}</AlertDialogTitle>
        <AlertDialogDescription>{{ t('exit_cdp.description') }}</AlertDialogDescription>
      </AlertDialogHeader>
      <AlertDialogFooter>
        <Button variant="outline" :disabled="state.checking || state.closing" @click="guard.closeOnly">
          {{ t('exit_cdp.close_only') }}
        </Button>
        <Button :disabled="state.checking || state.closing" @click="guard.restoreAndClose">
          <LoaderCircle v-if="state.checking" class="mr-2 h-4 w-4 animate-spin" />
          {{ t('exit_cdp.restore_and_close') }}
        </Button>
      </AlertDialogFooter>
    </AlertDialogContent>
  </AlertDialog>
</template>
