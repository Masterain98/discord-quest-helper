import { ref } from 'vue'
import {
  fetchSuperPropertiesCdp,
  getDebugInfo,
  getSuperPropertiesMode,
  retrySuperProperties,
  type DebugInfo,
  type SuperPropertiesModeInfo,
} from '@/api/tauri'

export function useSuperPropertiesDiagnostics() {
  const debugInfo = ref<DebugInfo | null>(null)
  const superPropsMode = ref<SuperPropertiesModeInfo | null>(null)
  let debugRevision = 0
  let modeRevision = 0

  async function loadDebugInfo() {
    const revision = ++debugRevision
    const result = await getDebugInfo()
    if (revision === debugRevision) debugInfo.value = result
  }

  async function fetchCdpPreview(port: number) {
    const revision = ++debugRevision
    const result = await fetchSuperPropertiesCdp(port)
    if (revision === debugRevision) debugInfo.value = result.debug_info
  }

  async function loadSuperPropsMode() {
    const revision = ++modeRevision
    const result = await getSuperPropertiesMode()
    if (revision === modeRevision) superPropsMode.value = result
  }

  async function retrySuperPropsMode(port: number) {
    const revision = ++modeRevision
    const result = await retrySuperProperties(port)
    if (revision === modeRevision) superPropsMode.value = result
  }

  return { debugInfo, superPropsMode, loadDebugInfo, fetchCdpPreview, loadSuperPropsMode, retrySuperPropsMode }
}
