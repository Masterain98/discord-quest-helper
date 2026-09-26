import { describe, expect, it } from 'vitest'
import type { CdpDiagnosticSnapshot } from '@/api/tauri'
import { sanitizeCdpDiagnosticExport } from './cdpDiagnostics'

describe('sanitizeCdpDiagnosticExport', () => {
  it('omits target details, full paths, sensitive params, emails, and Discord IDs', () => {
    const snapshot = {
      timestamp: '2026-09-26T00:00:00Z',
      port: 9223,
      endpointStatus: 'cdpWithoutDiscordTarget',
      endpointOwner: 'official',
      ownerProviderId: 'discord.official',
      selectedClient: 'Discord Stable',
      selectedInstallationId: 'secret-installation-id',
      selectedProviderId: 'discord.official',
      selectedVariantId: 'stable',
      selectedExecutablePath: 'C:\\Users\\Alice\\AppData\\Discord.exe',
      selectedRunning: true,
      portListening: true,
      cdpHttpReachable: true,
      cdpHttpStatus: 200,
      cdpResponseParseable: true,
      cdpTargetCount: 1,
      discordTargetCount: 1,
      mainRendererFound: false,
      processes: [{
        pid: 1,
        processName: 'Discord.exe',
        providerId: 'discord.official',
        installationId: 'secret-installation-id',
        executablePath: 'C:\\Users\\Alice\\AppData\\Discord.exe',
        isSelectedInstallation: true,
        hasRemoteDebuggingPortArg: true,
        remoteDebuggingPort: 9223,
        startTime: 1,
      }],
      targets: [{
        id: 'secret-target-id',
        type: 'page',
        title: 'Private channel',
        url: 'https://discord.com/channels/123456789012345678',
        hasWebSocketDebuggerUrl: true,
        isDiscordTarget: true,
        isAuxiliaryWindow: false,
        isMainRenderer: false,
        classification: 'discordOtherRenderer',
      }],
      lastLaunchError: null,
    } satisfies CdpDiagnosticSnapshot
    const exported = sanitizeCdpDiagnosticExport(snapshot, {
      code: 'cdp_readiness_timeout',
      message: 'Failed for Alice@example.com at C:\\Users\\Alice\\AppData\\Discord.exe user 123456789012345678',
      params: { port: 9223, path: 'C:\\Users\\Alice\\secret', authorization: 'Bearer secret' },
      rawType: 'object',
      timestamp: '2026-09-26T00:00:01Z',
    })
    const json = JSON.stringify(exported)
    expect(json).toContain('Discord.exe')
    expect(json).toContain('discordOtherRenderer')
    expect(json).not.toContain('Alice')
    expect(json).not.toContain('secret-installation-id')
    expect(json).not.toContain('secret-target-id')
    expect(json).not.toContain('Private channel')
    expect(json).not.toContain('123456789012345678')
    expect(json).not.toContain('authorization')
  })
})
