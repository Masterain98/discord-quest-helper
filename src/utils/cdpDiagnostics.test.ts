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

  it('redacts sensitive text in executable basenames and process names', () => {
    const snapshot = {
      timestamp: '2026-09-26T00:00:00Z',
      port: 9223,
      endpointStatus: 'unreachable',
      endpointOwner: 'none',
      ownerProviderId: null,
      selectedClient: null,
      selectedInstallationId: null,
      selectedProviderId: null,
      selectedVariantId: null,
      selectedExecutablePath: 'C:\\portable\\alice@example.com.exe',
      selectedRunning: false,
      portListening: false,
      cdpHttpReachable: false,
      cdpHttpStatus: null,
      cdpResponseParseable: false,
      cdpTargetCount: 0,
      discordTargetCount: 0,
      mainRendererFound: false,
      processes: [{
        pid: 42,
        processName: 'Bot top-secret',
        providerId: null,
        installationId: null,
        executablePath: '/opt/123456789012345678.exe',
        isSelectedInstallation: false,
        hasRemoteDebuggingPortArg: true,
        remoteDebuggingPort: 9223,
        startTime: 1,
      }],
      targets: [],
    } satisfies CdpDiagnosticSnapshot

    const json = JSON.stringify(sanitizeCdpDiagnosticExport(snapshot, null))
    expect(json).not.toContain('alice@example.com')
    expect(json).not.toContain('123456789012345678')
    expect(json).not.toContain('top-secret')
    expect(json).toContain('[redacted-email]')
    expect(json).toContain('[redacted-id]')
    expect(json).toContain('[redacted-authorization]')
  })

  it('validates diagnostic parameter values as well as their keys', () => {
    const snapshot = {
      timestamp: '2026-09-26T00:00:00Z', port: 9223,
      endpointStatus: 'unreachable', endpointOwner: 'none', ownerProviderId: null,
      selectedClient: null, selectedInstallationId: null, selectedProviderId: null,
      selectedVariantId: null, selectedExecutablePath: null, selectedRunning: false,
      portListening: false, cdpHttpReachable: false, cdpHttpStatus: null,
      cdpResponseParseable: false, cdpTargetCount: 0, discordTargetCount: 0,
      mainRendererFound: false, processes: [], targets: [],
    } satisfies CdpDiagnosticSnapshot
    const exported = sanitizeCdpDiagnosticExport(snapshot, {
      code: 'cdp_error', message: 'Connection failed', rawType: 'object',
      params: {
        port: 'Bearer secret', timeoutMs: 10000, lastStatus: 'Bearer secret',
        providerId: 'alice@example.com', mainRendererFound: false,
      },
      timestamp: '2026-09-26T00:00:01Z',
    })
    expect(exported.lastLaunchError?.params).toEqual({ timeoutMs: 10000, mainRendererFound: false })
    expect(JSON.stringify(exported)).not.toContain('secret')
    expect(JSON.stringify(exported)).not.toContain('alice@example.com')
  })
})
