import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useSuperPropertiesDiagnostics } from './superPropertiesDiagnostics'

const api = vi.hoisted(() => ({
  fetchSuperPropertiesCdp: vi.fn(),
  getDebugInfo: vi.fn(),
  getSuperPropertiesMode: vi.fn(),
  retrySuperProperties: vi.fn(),
}))

vi.mock('@/api/tauri', () => api)

describe('super properties diagnostics', () => {
  beforeEach(() => vi.resetAllMocks())

  it('displays the captured preview instead of reloading the unbound request identity', async () => {
    const preview = {
      source: 'Discord Client (CDP)',
      super_properties: { client_build_number: 123456, launch_signature: null, future_field: '中文' },
    }
    api.getDebugInfo.mockResolvedValue({ source: 'Default' })
    api.fetchSuperPropertiesCdp.mockResolvedValue({ debug_info: preview })
    const diagnostics = useSuperPropertiesDiagnostics()
    await diagnostics.loadDebugInfo()
    await diagnostics.fetchCdpPreview(9223)
    expect(diagnostics.debugInfo.value).toEqual(preview)
    expect(api.fetchSuperPropertiesCdp).toHaveBeenCalledWith(9223)
    expect(api.getDebugInfo).toHaveBeenCalledTimes(1)
  })

  it('does not let a late initial load replace a newer capture', async () => {
    let resolveInitial!: (value: unknown) => void
    api.getDebugInfo.mockReturnValue(new Promise(resolve => { resolveInitial = resolve }))
    const preview = { source: 'Discord Client (CDP)', super_properties: { client_build_number: 123456 } }
    api.fetchSuperPropertiesCdp.mockResolvedValue({ debug_info: preview })
    const diagnostics = useSuperPropertiesDiagnostics()
    const initial = diagnostics.loadDebugInfo()
    await diagnostics.fetchCdpPreview(9223)
    resolveInitial({ source: 'Default' })
    await initial
    expect(diagnostics.debugInfo.value).toEqual(preview)
  })

  it('uses the retry preview mode and ignores a late request-mode load', async () => {
    let resolveInitial!: (value: unknown) => void
    api.getSuperPropertiesMode.mockReturnValue(new Promise(resolve => { resolveInitial = resolve }))
    const preview = { success: true, mode: 'cdp', mode_display: 'Discord Client (CDP)', build_number: 123456 }
    api.retrySuperProperties.mockResolvedValue(preview)
    const diagnostics = useSuperPropertiesDiagnostics()
    const initial = diagnostics.loadSuperPropsMode()
    await diagnostics.retrySuperPropsMode(9223)
    resolveInitial({ mode: 'default', mode_display: 'Default', build_number: 1 })
    await initial
    expect(diagnostics.superPropsMode.value).toEqual(preview)
    expect(api.retrySuperProperties).toHaveBeenCalledWith(9223)
    expect(api.getSuperPropertiesMode).toHaveBeenCalledTimes(1)
  })
})
