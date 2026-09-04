import { computed, ref } from 'vue'
import {
  addDesktopClientInstallation,
  getDesktopClientState,
  removeDesktopClientInstallation,
  setDesktopClientSelection,
  type ClientInstallation,
  type ClientSelection,
  type DesktopClientArg,
  type DesktopClientState,
  type ProviderId,
} from '@/api/tauri'

const state = ref<DesktopClientState | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
let latestRequest = 0

function errorMessage(value: unknown): string {
  if (typeof value === 'object' && value && 'message' in value) return String(value.message)
  return value instanceof Error ? value.message : String(value)
}

async function apply(
  operation: () => Promise<DesktopClientState>,
  port: number,
): Promise<DesktopClientState> {
  const request = ++latestRequest
  loading.value = true
  error.value = null
  try {
    const snapshot = await operation()
    if (request === latestRequest && snapshot.port === port) state.value = snapshot
    return snapshot
  } catch (cause) {
    if (request === latestRequest) error.value = errorMessage(cause)
    throw cause
  } finally {
    if (request === latestRequest) loading.value = false
  }
}

async function refresh(port: number): Promise<DesktopClientState | null> {
  try {
    return await apply(() => getDesktopClientState(port), port)
  } catch {
    return null
  }
}

async function select(selection: ClientSelection, port: number): Promise<DesktopClientState> {
  return apply(() => setDesktopClientSelection(selection, port), port)
}

async function addInstallation(providerId: ProviderId, path: string, port: number) {
  return apply(() => addDesktopClientInstallation(providerId, path, port), port)
}

async function removeInstallation(installationId: string, port: number) {
  return apply(() => removeDesktopClientInstallation(installationId, port), port)
}

async function migrateLegacySelection(port: number, legacy: DesktopClientArg) {
  const marker = 'questHelper_desktopClientMigratedV1'
  if (localStorage.getItem(marker) === 'true') return
  const snapshot = state.value ?? await refresh(port)
  if (!snapshot) return
  if (snapshot.selection.kind === 'auto' && legacy !== 'auto') {
    const providerId: ProviderId = legacy === 'vesktop' ? 'vencord.vesktop' : 'discord.official'
    await select({ kind: 'provider', providerId, variantId: null }, port)
  }
  localStorage.setItem(marker, 'true')
}

export function desktopClientArgForProvider(providerId: ProviderId): DesktopClientArg {
  if (providerId === 'vencord.vesktop') return 'vesktop'
  if (providerId === 'discord.official') return 'official'
  return 'auto'
}

function installationPath(installation: ClientInstallation | null | undefined): string | undefined {
  if (!installation) return undefined
  if (installation.launchTarget.kind === 'executable') return installation.launchTarget.path
  if (installation.launchTarget.kind === 'macBundle') return installation.launchTarget.executablePath
  return undefined
}

function providerForSelection(selection: ClientSelection | null | undefined): ProviderId | null {
  if (!selection || selection.kind === 'auto') return null
  if (selection.kind === 'provider') return selection.providerId
  return state.value?.installations.find(item => item.id === selection.installationId)?.providerId ?? null
}

export function useDesktopClientState() {
  const selectedInstallation = computed(() => {
    const selection = state.value?.selection
    if (selection?.kind !== 'installation') return null
    return state.value?.installations.find(item => item.id === selection.installationId) ?? null
  })
  const selectedProviderId = computed(() => providerForSelection(state.value?.selection))
  const selectedIsRunning = computed(() => {
    const snapshot = state.value
    if (!snapshot) return false
    const selection = snapshot.selection
    if (selection.kind === 'installation') {
      return snapshot.processes.some(process => process.installationId === selection.installationId)
    }
    if (selection.kind === 'provider') {
      return snapshot.processes.some(process => process.providerId === selection.providerId)
    }
    return snapshot.processes.length > 0
  })
  return {
    state,
    loading,
    error,
    selectedInstallation,
    selectedProviderId,
    selectedIsRunning,
    refresh,
    select,
    addInstallation,
    removeInstallation,
    migrateLegacySelection,
    installationPath,
  }
}
